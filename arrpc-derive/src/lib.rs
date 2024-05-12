#[cfg(feature = "obake")]
mod obake;
mod util;

use convert_case::{Case, Casing};
use itertools::Itertools;
use proc_macro::TokenStream;
use proc_macro2::Span;
use proc_macro_error::proc_macro_error;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote, Arm, FnArg, Ident, ItemEnum, ItemImpl, ItemTrait, Meta,
    ReturnType, TraitItem, TraitItemFn, Type, Variant,
};

type FlagProcessor = fn(ArrpcImpls) -> ArrpcImpls;

const PROC_VAR: &str = "proc";

#[proc_macro_error]
#[proc_macro_attribute]
pub fn arrpc_service(attr: TokenStream, item: TokenStream) -> TokenStream {
    let flag_processors = {
        let mut processors: Vec<FlagProcessor> = Vec::new();

        #[cfg(feature = "obake")]
        processors.push(obake::processor);

        processors
    };

    let original_trait = parse_macro_input!(item as ItemTrait);
    let mut svc_trait = original_trait.clone();
    let svc_name = &svc_trait.ident;

    let proc_name = Ident::new(format!("{svc_name}Proc").as_str(), Span::call_site());

    let mut proc_variants = Vec::new();

    for item in svc_trait.items.iter_mut() {
        if let TraitItem::Fn(trait_fn) = item {
            trait_fn.sig.output = wrap_with_arrpc_result(&trait_fn.sig.output);

            let proc = create_proc_variant(trait_fn);

            let proc_match = match_for_proc_variant(&proc, &proc_name, &trait_fn.sig.ident);

            let impl_fn = create_client_impl(&proc, &trait_fn, &proc_name);

            let proc_variant = ProcVariant {
                variant: proc,
                svc_match_stmt: proc_match,
                client_impl: impl_fn,
            };

            proc_variants.push(proc_variant);
        }
    }

    let procs = proc_variants.iter().map(|proc| &proc.variant).collect_vec();
    let proc_enum: ItemEnum = parse_quote! {
        #[derive(serde::Serialize, serde::Deserialize)]
        enum #proc_name {
            #(#procs),*
        }
    };

    let proc_var = proc_var_ident();

    // Create arrpc_service impl
    let svc_impl = parse_macro_input!(attr as Type);
    let proc_matches = proc_variants
        .iter()
        .map(|proc| &proc.svc_match_stmt)
        .collect_vec();
    let arrpc_svc_impl: ItemImpl = parse_quote! {
        #[async_trait::async_trait]
        impl arrpc::core::Service for #svc_impl {
            async fn accept<R>(&self, req: R) -> arrpc::core::Result<R::Response>
            where
                R: arrpc::core::Request + Send + Sync,
            {
                let #proc_var: #proc_name = req.proc()?;
                match #proc_var {
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

    let fn_impls = proc_variants
        .iter()
        .map(|proc| &proc.client_impl)
        .collect_vec();
    let unv_client_impl: ItemImpl = parse_quote! {
        #async_attr
        impl<T> #svc_name for arrpc::core::UniversalClient<T>
            where T: arrpc::core::ClientContract + Send + Sync
        {
            #(#fn_impls)*
        }
    };

    let mut impls = ArrpcImpls {
        updated_trait: svc_trait,
        proc_enum,
        svc_impl: arrpc_svc_impl,
        client_impl: unv_client_impl,
        extras: Vec::new(),
    };

    for processor in flag_processors {
        impls = processor(impls);
    }

    impls.into()
}

fn proc_var_ident() -> Ident {
    Ident::new(PROC_VAR, Span::call_site())
}

fn wrap_with_arrpc_result(old: &ReturnType) -> ReturnType {
    let ret_type = match old {
        ReturnType::Default => quote!(()),
        ReturnType::Type(_, ret_type) => quote!(#ret_type),
    };

    let replacement_ret: ReturnType = parse_quote! {
        -> arrpc::core::Result<#ret_type>
    };

    replacement_ret
}

fn create_proc_variant(trait_fn: &TraitItemFn) -> Variant {
    let fn_name = &trait_fn.sig.ident;
    let name = proc_name_for_fn(fn_name.to_string().as_str());
    let name: Ident = Ident::new(name.as_str(), Span::call_site());
    let args = trait_fn
        .sig
        .inputs
        .iter()
        .filter(|input| matches!(**input, FnArg::Typed(_)));
    let proc: Variant = parse_quote! {
        #name {
             #(#args),*
        }
    };

    proc
}

fn proc_name_for_fn(fn_name: &str) -> String {
    fn_name.from_case(Case::Snake).to_case(Case::Pascal)
}

fn match_for_proc_variant(proc_variant: &Variant, proc_name: &Ident, fn_name: &Ident) -> Arm {
    let args = proc_variant
        .fields
        .iter()
        .filter_map(|field| field.ident.as_ref())
        .collect_vec();
    let name = &proc_variant.ident;

    parse_quote!(#proc_name::#name{#(#args),*} => req.respond(self.#fn_name(#(#args),*).await?))
}

fn create_client_impl(
    proc_variant: &Variant,
    trait_fn: &TraitItemFn,
    proc_name: &Ident,
) -> TraitItemFn {
    let TraitItemFn { sig, .. } = trait_fn;
    let name = &proc_variant.ident;
    let args = proc_variant
        .fields
        .iter()
        .filter_map(|field| field.ident.as_ref())
        .collect_vec();
    let proc_var = proc_var_ident();
    parse_quote! {
        #sig {
            let #proc_var = #proc_name::#name{#(#args),*};
            self.0
                .send(#proc_var)
                .await
        }
    }
}

struct ArrpcImpls {
    pub updated_trait: ItemTrait,
    pub proc_enum: ItemEnum,
    pub svc_impl: ItemImpl,
    pub client_impl: ItemImpl,
    pub extras: Vec<proc_macro2::TokenStream>,
}

impl From<ArrpcImpls> for TokenStream {
    fn from(value: ArrpcImpls) -> Self {
        let ArrpcImpls {
            updated_trait,
            proc_enum,
            svc_impl,
            client_impl,
            extras,
            ..
        } = value;

        quote! {
            #updated_trait

            #proc_enum

            #svc_impl

            #client_impl

            #(#extras)*
        }
        .into()
    }
}

struct ProcVariant {
    variant: Variant,
    svc_match_stmt: Arm,
    client_impl: TraitItemFn,
}
