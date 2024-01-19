use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote, FnArg, Ident, ItemTrait, Meta, ReturnType, TraitItem,
    TraitItemFn, Type,
};

#[proc_macro_attribute]
pub fn arrpc_service(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut svc_trait = parse_macro_input!(item as ItemTrait);
    let svc_name = &svc_trait.ident;

    let proc_name = Ident::new(format!("{}Proc", svc_name).as_str(), Span::call_site());

    let mut procs = Vec::new();
    let mut proc_matches = Vec::new();
    let mut fn_impls = Vec::new();

    for item in svc_trait.items.iter_mut() {
        if let TraitItem::Fn(trait_fn) = item {
            // Create enum variant
            let fn_name = &trait_fn.sig.ident;
            let name = fn_name
                .to_string()
                .from_case(Case::Snake)
                .to_case(Case::Pascal);
            let name: Ident = Ident::new(name.as_str(), Span::call_site());
            let args = trait_fn
                .sig
                .inputs
                .iter()
                .filter(|input| matches!(**input, FnArg::Typed(_)));
            let proc = quote! {
                #name {
                     #(#args),*
                }
            };
            procs.push(proc);

            // Create match statement for proc
            let args = trait_fn
                .sig
                .inputs
                .iter()
                .filter_map(|input| match input {
                    FnArg::Receiver(_) => None,
                    FnArg::Typed(pat_type) => Some(pat_type.pat.clone()),
                })
                .collect::<Vec<_>>();
            let proc_match = quote!(#proc_name::#name{#(#args),*} => req.respond(self.#fn_name(#(#args),*).await?));
            proc_matches.push(proc_match);

            // Wrap return with lib Result
            let ret_type = match &trait_fn.sig.output {
                ReturnType::Default => quote!(()),
                ReturnType::Type(_, ret_type) => quote!(#ret_type),
            };

            let replacement_ret: ReturnType = parse_quote! {
                -> arrpc_core::Result<#ret_type>
            };
            trait_fn.sig.output = replacement_ret;

            // Create sig without default or semi-colon
            let TraitItemFn { attrs, sig, .. } = trait_fn.clone();
            let impl_fn = quote! {
                #(#attrs)*
                #sig {
                    self.0
                        .send(#proc_name::#name{#(#args),*})
                        .await
                }
            };
            fn_impls.push(impl_fn);
        }
    }

    let proc_enum = quote! {
        #[derive(serde::Serialize, serde::Deserialize)]
        enum #proc_name {
            #(#procs),*
        }
    };

    // Create arrpc_service impl
    let svc_impl = parse_macro_input!(attr as Type);
    let arrpc_svc_impl = quote! {
        #[async_trait::async_trait]
        impl arrpc_core::Service for #svc_impl {
            async fn accept<R>(&self, req: R) -> Result<R::Response>
            where
                R: arrpc_core::Request + Send + Sync,
            {
                let proc: #proc_name = req.proc()?;
                match proc {
                    #(#proc_matches),*
                }
            }
        }
    };

    // Impl user svc for UniversalClient
    let async_attr = svc_trait.attrs.iter().find(|attr| match &attr.meta {
        Meta::Path(path) => path
            .segments
            .iter()
            .any(|segment| segment.ident == "async_trait"),
        _ => false,
    });

    let unv_client_impl = quote! {
        #async_attr
        impl<T> #svc_name for arrpc_core::UniversalClient<T>
            where T: arrpc_core::ClientContract + Send + Sync
        {
            #(#fn_impls)*
        }
    };

    quote! {
        #svc_trait

        #proc_enum

        #arrpc_svc_impl

        #unv_client_impl
    }
    .into()
}
