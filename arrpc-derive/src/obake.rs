use std::{collections::HashMap, fmt::Debug};

use itertools::Itertools;
use proc_macro2::TokenStream;
use proc_macro_error::emit_error;
use quote::ToTokens;
use semver::{Version, VersionReq};
use syn::{
    parse_quote, punctuated::Punctuated, token::Comma, Arm, Attribute, FieldValue, FnArg, Ident,
    ImplItem, ItemEnum, ItemImpl, LitStr, Meta, MetaList, Pat, Stmt, TraitItem, Variant,
};

use crate::{proc_name_for_fn, proc_var_ident, ArrpcImpls};

const OBAKE: &str = "obake";

pub fn processor(mut arrpc_impls: ArrpcImpls) -> ArrpcImpls {
    let trait_attrs;
    (arrpc_impls.updated_trait.attrs, trait_attrs) =
        remove_obake_attrs(arrpc_impls.updated_trait.attrs);

    if trait_attrs.versioned.is_none() {
        return arrpc_impls;
    }

    let proc_vers = trait_attrs
        .versions
        .to_owned()
        .into_iter()
        .map(|(_, ver)| ver)
        .sorted()
        .collect_vec();

    // Move obake attrs on Trait to Proc
    let mut attrs = Vec::new();
    add_obake_attrs(&mut attrs, trait_attrs);
    attrs.insert(
        1,
        parse_quote!(#[obake(derive(serde::Serialize, serde::Deserialize))]),
    );
    attrs.append(&mut arrpc_impls.proc_enum.attrs);
    arrpc_impls.proc_enum.attrs = attrs;

    arrpc_impls = apply_obake_constraints(
        arrpc_impls,
        proc_vers.last().expect("lastest version of proc"),
    );

    arrpc_impls.client_impl =
        adjust_client_impls(arrpc_impls.client_impl, &arrpc_impls.proc_enum.ident);

    arrpc_impls.svc_impl = adjust_service_impls(arrpc_impls.svc_impl);

    let mut migrations = generate_migrations(&arrpc_impls.proc_enum, &proc_vers);
    arrpc_impls.extras.append(&mut migrations);

    arrpc_impls
}

#[derive(Default)]
struct ObakeAttrs {
    pub versioned: Option<Attribute>,
    pub versions: Vec<(Attribute, Version)>,
    pub cfg: Vec<(Attribute, VersionReq)>,
    pub others: Vec<Attribute>,
}

impl Debug for ObakeAttrs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObakeAttrs")
            .field(
                "versioned",
                &self
                    .versioned
                    .as_ref()
                    .map(|versioned| versioned.to_token_stream().to_string()),
            )
            .field(
                "versions",
                &self
                    .versions
                    .iter()
                    .map(|(attr, ver)| (attr.to_token_stream().to_string(), ver))
                    .collect_vec(),
            )
            .field(
                "cfg",
                &self
                    .cfg
                    .iter()
                    .map(|(attr, cfg)| (attr.to_token_stream().to_string(), cfg))
                    .collect_vec(),
            )
            .field(
                "others",
                &self
                    .others
                    .iter()
                    .map(|attr| attr.to_token_stream().to_string())
                    .collect_vec(),
            )
            .finish()
    }
}

fn remove_obake_attrs(attrs: Vec<Attribute>) -> (Vec<Attribute>, ObakeAttrs) {
    let mut obake_attrs = ObakeAttrs::default();
    let mut without_obake = Vec::new();

    for attr in attrs {
        match &attr.meta {
            Meta::Path(path) => {
                match path.to_token_stream().to_string() == format!("{OBAKE} :: versioned") {
                    true => {
                        obake_attrs.versioned = Some(attr);
                    }
                    false => {
                        without_obake.push(attr);
                    }
                }
            }
            Meta::List(list) => match list.path.is_ident(OBAKE) {
                true => {
                    let nested_list: MetaList = list.parse_args().expect("nested obake meta list");
                    if nested_list.path.is_ident("version") {
                        let expr: LitStr = nested_list.parse_args().expect("semver expression");
                        let version =
                            Version::parse(expr.value().as_str()).expect("valid semver expression");
                        obake_attrs.versions.push((attr, version));
                    } else if nested_list.path.is_ident("cfg") {
                        let expr: LitStr = nested_list.parse_args().expect("semver req expression");
                        let version_req = VersionReq::parse(expr.value().as_str())
                            .expect("valid semver req expression");
                        obake_attrs.cfg.push((attr, version_req));
                    } else {
                        obake_attrs.others.push(attr);
                    }
                }
                false => {
                    without_obake.push(attr);
                }
            },
            _ => {
                without_obake.push(attr);
            }
        };
    }

    (without_obake, obake_attrs)
}

fn apply_obake_constraints(mut arrpc_impls: ArrpcImpls, latest_version: &Version) -> ArrpcImpls {
    fn find_proc_variant<'a>(proc: &'a mut ItemEnum, fn_name: &Ident) -> Option<&'a mut Variant> {
        let proc_name = proc_name_for_fn(fn_name.to_string().as_str());
        proc.variants
            .iter_mut()
            .find(|variant| variant.ident == proc_name)
    }

    let mut updated_items = Vec::new();
    for item in arrpc_impls.updated_trait.items {
        let item = match item {
            TraitItem::Fn(mut trait_fn) => {
                let fn_attrs;
                (trait_fn.attrs, fn_attrs) = remove_obake_attrs(trait_fn.attrs);

                let constraints = fn_attrs
                    .cfg
                    .to_owned()
                    .into_iter()
                    .map(|(_, req)| req)
                    .collect_vec();

                let variant = find_proc_variant(&mut arrpc_impls.proc_enum, &trait_fn.sig.ident)
                    .expect("corresponding proc for trait fn");

                add_obake_attrs(&mut variant.attrs, fn_attrs);

                let args = trait_fn
                    .sig
                    .inputs
                    .into_iter()
                    .flat_map(|arg| match arg {
                        FnArg::Typed(mut arg) => {
                            let arg_attrs;
                            (arg.attrs, arg_attrs) = remove_obake_attrs(arg.attrs);

                            let constraints = arg_attrs
                                .cfg
                                .to_owned()
                                .into_iter()
                                .map(|(_, cfg)| cfg)
                                .collect_vec();

                            if let Some(variant_arg) =
                                variant
                                    .fields
                                    .iter_mut()
                                    .find(|field| match arg.pat.as_ref() {
                                        syn::Pat::Ident(arg) => {
                                            field.ident.as_ref() == Some(&arg.ident)
                                        }
                                        _ => false,
                                    })
                            {
                                add_obake_attrs(&mut variant_arg.attrs, arg_attrs);
                            }

                            match ver_constraint_met(&constraints, latest_version) {
                                false => None,
                                true => Some(FnArg::Typed(arg)),
                            }
                        }
                        other => Some(other),
                    })
                    .collect_vec();

                trait_fn.sig.inputs = parse_quote!(#(#args),*);

                match ver_constraint_met(&constraints, latest_version) {
                    false => None,
                    true => Some(TraitItem::Fn(trait_fn)),
                }
            }
            other => Some(other),
        };

        if let Some(item) = item {
            updated_items.push(item);
        }
    }

    arrpc_impls.updated_trait.items = updated_items;

    arrpc_impls
}

fn add_obake_attrs(
    attrs: &mut Vec<Attribute>,
    ObakeAttrs {
        versioned,
        versions,
        cfg,
        mut others,
    }: ObakeAttrs,
) {
    if let Some(versioned) = versioned {
        attrs.push(versioned);
    }

    for version in versions.into_iter().map(|(version, _)| version) {
        attrs.push(version);
    }

    for cfg in cfg.into_iter().map(|(cfg, _)| cfg) {
        attrs.push(cfg);
    }

    attrs.append(&mut others);
}

fn adjust_client_impls(mut client_impl: ItemImpl, proc_name: &Ident) -> ItemImpl {
    let proc_var = proc_var_ident();
    client_impl.items = client_impl
        .items
        .into_iter()
        .map(|item| match item {
            ImplItem::Fn(mut fn_item) => {
                fn_item.block.stmts.insert(
                    1,
                    parse_quote!(let #proc_var: obake::AnyVersion<#proc_name> = #proc_var.into();),
                );
                ImplItem::Fn(fn_item)
            }
            other => other,
        })
        .collect_vec();

    client_impl
}

fn adjust_service_impls(mut svc_impl: ItemImpl) -> ItemImpl {
    let proc_var = proc_var_ident();
    svc_impl.items = svc_impl
        .items
        .into_iter()
        .map(|item| match item {
            ImplItem::Fn(mut fn_item) => {
                let mut stmts = fn_item.block.stmts;
                let assignment = stmts.remove(0);
                let assignment = match assignment {
                    Stmt::Local(assignment) => assignment,
                    _ => unreachable!("svc impl proc assignment was generated within macro"),
                };

                let proc = match &assignment.pat {
                    Pat::Type(pat_type) => pat_type.ty.to_owned(),
                    _ => unreachable!("unexpected assignment pat"),
                };
                let init = assignment.init.expect("proc init statement").expr;
                stmts.insert(
                    0,
                    parse_quote!(let #proc_var: obake::AnyVersion<#proc> = #init;),
                );
                stmts.insert(1, parse_quote!(let #proc_var: #proc = #proc_var.into();));

                fn_item.block.stmts = stmts;
                ImplItem::Fn(fn_item)
            }
            other => other,
        })
        .collect_vec();

    svc_impl
}

fn generate_migrations(proc_enum: &ItemEnum, versions: &Vec<Version>) -> Vec<TokenStream> {
    fn apply_constraint<'a>(
        proc: &'a ItemEnum,
        version: &Version,
    ) -> HashMap<&'a Ident, Vec<&'a Ident>> {
        let mut res = HashMap::new();
        for proc_var in proc.variants.iter() {
            let (_, ObakeAttrs { cfg, .. }) = remove_obake_attrs(proc_var.attrs.to_owned());

            let constraints = cfg.iter().map(|(_, ver)| ver.to_owned()).collect_vec();
            if !ver_constraint_met(&constraints, version) {
                // This function does not exist for this version
                continue;
            }

            let proc_name = &proc_var.ident;

            // Will need to reduce once constraints are supported on fields
            let args = proc_var
                .fields
                .iter()
                .map(|field| {
                    field
                        .ident
                        .as_ref()
                        .expect("proc variant fields should all be named")
                })
                .collect_vec();

            res.insert(proc_name, args);
        }

        res
    }

    let proc_enum_name = &proc_enum.ident;
    let versions = versions.iter().sorted().collect_vec();

    let mut migrations = Vec::new();
    for i in 0..versions.len() - 1 {
        let before = versions[i];
        let after = versions[i + 1];

        let before_procs = apply_constraint(&proc_enum, &before);
        let after_procs = apply_constraint(&proc_enum, &after);

        let mut match_arms = Vec::new();
        for (proc_name, before_args) in before_procs {
            let after_args = after_procs.get(proc_name).expect("function not deleted");
            if let Some(removed_arg) = before_args
                .iter()
                .find(|before_arg| !after_args.contains(before_arg))
            {
                emit_error!(
                    removed_arg.span(),
                    "argument has been removed which is not currently supported"
                );
            }

            let new_args = after_args
                .iter()
                .filter(|after_arg| !before_args.contains(after_arg))
                .collect_vec();

            let before_args: Punctuated<FieldValue, Comma> = parse_quote!(#(#before_args),*);
            let mut after_args: Vec<FieldValue> = after_args
                .iter()
                .map(|arg| parse_quote!(#arg))
                .collect_vec();

            after_args.append(
                &mut new_args
                    .iter()
                    .map(|arg| parse_quote!(#arg: Default::default()))
                    .collect_vec(),
            );

            let match_arm: Arm = parse_quote!(Prev::#proc_name { #before_args } => Self::#proc_name { #(#after_args),* });
            match_arms.push(match_arm);
        }

        let before = before.to_string();
        let after = after.to_string();
        let migration: ItemImpl = parse_quote! {
            impl From<#proc_enum_name![#before]> for #proc_enum_name![#after] {
                fn from(value: #proc_enum_name![#before]) -> Self {
                    type Prev = #proc_enum_name![#before];
                    match value {
                        #(#match_arms),*
                    }
                }
            }
        };

        migrations.push(migration.to_token_stream());
    }

    migrations
}

fn ver_constraint_met(constraints: &Vec<VersionReq>, version: &Version) -> bool {
    constraints.is_empty() || constraints.iter().any(|req| req.matches(version))
}
