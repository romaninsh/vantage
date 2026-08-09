//! `#[actions]` — generate [`ModelAction`] constructors from a trait.
//!
//! Placed on a trait (above `#[async_trait]`), it scans for methods
//! marked `#[action(...)]` and emits one constructor function per
//! action, next to the trait. The trait itself is re-emitted unchanged
//! (minus the marker attributes), so the business logic stays an
//! ordinary extension-trait method.
//!
//! Parameter classification, by role:
//!
//! - `&self` — the target the constructor captures (any implementor of
//!   the trait: a `Table<SurrealDB, Tag>`, a mock store in tests, …);
//! - `#[id] name: &str` — the record key a transport auto-fills (path
//!   segment, selected row, rhai row argument). Present → record
//!   action; absent → table action;
//! - exactly one owned parameter — the serialized input (must be
//!   `Deserialize + JsonSchema`);
//! - remaining `&T` parameters — dependencies; each becomes an
//!   `Arc<T>` constructor argument captured into the action.
//!
//! ```ignore
//! #[actions]
//! #[async_trait]
//! pub trait TagRegistration {
//!     /// Register this tag to an owner.
//!     #[action(name = "register_tag", table = "tag")]
//!     async fn register(&self, #[id] tag_id: &str, req: RegistrationRequest, mail: &Mailer)
//!         -> Result<RegistrationStarted>;
//! }
//! // generates:
//! // pub fn register_tag_action<S: TagRegistration + Send + Sync + 'static>(
//! //     target: S, mail: Arc<Mailer>) -> Arc<dyn ModelAction>
//! ```
//!
//! The generated constructor calls `vantage_action::record_action` /
//! `table_action` — the adapters remain the foundation; drop to them
//! directly for signatures this classification cannot express.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{FnArg, Ident, ItemTrait, LitStr, Pat, TraitItem, Type, parse_macro_input};

#[proc_macro_attribute]
pub fn actions(_args: TokenStream, input: TokenStream) -> TokenStream {
    let mut item = parse_macro_input!(input as ItemTrait);
    let trait_name = item.ident.clone();

    let mut constructors: Vec<TokenStream2> = Vec::new();
    let mut errors: Vec<syn::Error> = Vec::new();

    // The constructor is emitted as `fn …<S> where S: Trait`, which has
    // nowhere to put the trait's own parameters. Say so here rather than
    // letting rustc report `missing generics for trait` against a span
    // inside generated code.
    if item
        .generics
        .params
        .iter()
        .any(|p| !matches!(p, syn::GenericParam::Lifetime(_)))
    {
        errors.push(syn::Error::new_spanned(
            &item.generics,
            "#[actions] does not support generic traits — the generated constructors have \
             nowhere to carry the parameters; move the generic onto a dependency argument",
        ));
    }

    for entry in &mut item.items {
        let TraitItem::Fn(method) = entry else {
            continue;
        };
        // Take the #[action(...)] attribute off the method, if any.
        let Some(pos) = method
            .attrs
            .iter()
            .position(|a| a.path().is_ident("action"))
        else {
            continue;
        };
        let attr = method.attrs.remove(pos);

        match parse_action(&trait_name, attr, method) {
            Ok(tokens) => constructors.push(tokens),
            Err(e) => errors.push(e),
        }
    }

    let errors = errors.into_iter().map(|e| e.to_compile_error());
    quote! {
        #item
        #(#constructors)*
        #(#errors)*
    }
    .into()
}

struct ActionAttr {
    name: Option<String>,
    table: Option<String>,
}

fn parse_attr(attr: &syn::Attribute) -> syn::Result<ActionAttr> {
    let mut out = ActionAttr {
        name: None,
        table: None,
    };
    attr.parse_nested_meta(|meta| {
        let value: LitStr = meta.value()?.parse()?;
        if meta.path.is_ident("name") {
            out.name = Some(value.value());
        } else if meta.path.is_ident("table") {
            out.table = Some(value.value());
        } else {
            return Err(meta.error("expected `name = \"…\"` or `table = \"…\"`"));
        }
        Ok(())
    })?;
    Ok(out)
}

/// `true` for `&str` (with or without a named lifetime).
fn is_str_ref(ty: &Type) -> bool {
    let Type::Reference(r) = ty else {
        return false;
    };
    matches!(&*r.elem, Type::Path(p) if p.qself.is_none() && p.path.is_ident("str"))
}

/// Doc comments on the method, joined into the action description.
fn doc_description(method: &syn::TraitItemFn) -> String {
    let mut lines = Vec::new();
    for attr in &method.attrs {
        if attr.path().is_ident("doc")
            && let syn::Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
        {
            lines.push(s.value().trim().to_string());
        }
    }
    lines.join("\n").trim().to_string()
}

fn parse_action(
    trait_name: &Ident,
    attr: syn::Attribute,
    method: &mut syn::TraitItemFn,
) -> syn::Result<TokenStream2> {
    // Strip the `#[id]` markers first, recording where they were. They
    // are inert markers rather than real attributes, so a method we bail
    // out of below would otherwise carry them into the re-emitted trait
    // and bury the real diagnostic under "cannot find attribute `id`".
    let id_flags: Vec<bool> = method
        .sig
        .inputs
        .iter_mut()
        .map(|arg| match arg {
            FnArg::Typed(pat_type) => {
                let had_id = pat_type.attrs.iter().any(|a| a.path().is_ident("id"));
                pat_type.attrs.retain(|a| !a.path().is_ident("id"));
                had_id
            }
            FnArg::Receiver(_) => false,
        })
        .collect();

    let spec = parse_attr(&attr)?;
    let method_name = method.sig.ident.clone();
    let action_name = spec.name.unwrap_or_else(|| method_name.to_string());
    let table = spec
        .table
        .ok_or_else(|| syn::Error::new_spanned(&method.sig, "#[action] requires table = \"…\""))?;
    let description = doc_description(method);

    // The constructor name is derived from the action name, so the
    // action name has to be a legal identifier. Without this check
    // `format_ident!` panics the compiler with "`register-tag_action` is
    // not a valid identifier" and no span to point at.
    if syn::parse_str::<Ident>(&action_name).is_err() {
        return Err(syn::Error::new_spanned(
            &attr,
            format!(
                "#[action] name `{action_name}` is not a valid Rust identifier, so no \
                 `{action_name}_action` constructor can be generated — use snake_case here \
                 and let the transport map it to whatever the wire needs"
            ),
        ));
    }

    // Classify parameters. Call-site arguments are rebuilt in declared
    // order so the generated call matches the method signature exactly.
    let mut id_param: Option<Ident> = None;
    let mut input: Option<(Ident, Type)> = None;
    let mut deps: Vec<(Ident, Type)> = Vec::new();
    let mut call_args: Vec<TokenStream2> = Vec::new();
    let mut saw_receiver = false;

    for (arg, had_id) in method.sig.inputs.iter_mut().zip(&id_flags) {
        let had_id = *had_id;
        match arg {
            FnArg::Receiver(receiver) => {
                // The constructor puts the target behind an `Arc`, so a
                // `&mut self` body could never be called. Rejecting it
                // here beats `cannot borrow data in an Arc as mutable`
                // pointing into generated code.
                if receiver.mutability.is_some() || receiver.reference.is_none() {
                    return Err(syn::Error::new_spanned(
                        receiver,
                        "#[action] methods must take `&self` — the target is shared behind an Arc",
                    ));
                }
                saw_receiver = true;
            }
            FnArg::Typed(pat_type) => {
                let name = match &*pat_type.pat {
                    Pat::Ident(p) => p.ident.clone(),
                    other => {
                        return Err(syn::Error::new_spanned(
                            other,
                            "#[action] methods need simple named parameters",
                        ));
                    }
                };
                // Parameter types are copied verbatim into a free
                // function, where `Self` does not exist. Catching it
                // here beats `cannot find type `Self` in this scope`
                // pointing at generated code.
                if quote!(#pat_type).to_string().contains("Self") {
                    return Err(syn::Error::new_spanned(
                        &pat_type.ty,
                        "#[action] parameter types may not mention `Self` — the generated \
                         constructor is a free function; name the concrete type instead",
                    ));
                }

                if had_id {
                    if id_param.is_some() {
                        return Err(syn::Error::new_spanned(
                            pat_type,
                            "#[action] method has more than one #[id] parameter",
                        ));
                    }
                    // The transport hands over a `String` and the call
                    // site passes `&__id`, so the only signature that
                    // works is `&str`. Anything else fails as a
                    // mismatched-types error inside generated code.
                    if !is_str_ref(&pat_type.ty) {
                        return Err(syn::Error::new_spanned(
                            &pat_type.ty,
                            "#[id] parameters must be `&str` — the transport resolves the \
                             record key as a string",
                        ));
                    }
                    id_param = Some(name);
                    call_args.push(quote! { &__id });
                } else if let Type::Reference(r) = &*pat_type.ty {
                    if r.mutability.is_some() {
                        return Err(syn::Error::new_spanned(
                            pat_type,
                            "#[action] dependencies must be shared references — they are \
                             captured as `Arc<T>` and shared across invocations",
                        ));
                    }
                    let inner = (*r.elem).clone();
                    call_args.push(quote! { &#name });
                    deps.push((name, inner));
                } else {
                    if input.is_some() {
                        return Err(syn::Error::new_spanned(
                            pat_type,
                            "#[action] method has more than one owned (input) parameter; \
                             pass dependencies by reference",
                        ));
                    }
                    let ty = (*pat_type.ty).clone();
                    call_args.push(quote! { __input });
                    input = Some((name, ty));
                }
            }
        }
    }

    if !saw_receiver {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "#[action] methods must take &self (the table the action runs on)",
        ));
    }
    let (_, input_ty) = input.ok_or_else(|| {
        syn::Error::new_spanned(
            &method.sig,
            "#[action] methods need exactly one owned input parameter",
        )
    })?;

    let ctor_name = format_ident!("{}_action", action_name);
    let dep_params = deps.iter().map(|(name, ty)| {
        quote! { #name: ::std::sync::Arc<#ty> }
    });
    let dep_clones = deps.iter().map(|(name, _)| {
        quote! { let #name = ::std::sync::Arc::clone(&#name); }
    });
    let doc = format!(
        "Wrap [`{trait_name}::{method_name}`] as the `{action_name}` [ModelAction]\
         (vantage_action::ModelAction)."
    );

    let adapter_call = if id_param.is_some() {
        quote! {
            ::vantage_action::record_action(
                #action_name,
                #table,
                #description,
                move |__id: ::std::string::String, __input: #input_ty| {
                    let __target = ::std::sync::Arc::clone(&__target);
                    #(#dep_clones)*
                    async move { __target.#method_name(#(#call_args),*).await }
                },
            )
        }
    } else {
        quote! {
            ::vantage_action::table_action(
                #action_name,
                #table,
                #description,
                move |__input: #input_ty| {
                    let __target = ::std::sync::Arc::clone(&__target);
                    #(#dep_clones)*
                    async move { __target.#method_name(#(#call_args),*).await }
                },
            )
        }
    };

    Ok(quote! {
        #[doc = #doc]
        pub fn #ctor_name<S>(
            target: S,
            #(#dep_params),*
        ) -> ::std::sync::Arc<dyn ::vantage_action::ModelAction>
        where
            S: #trait_name + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let __target = ::std::sync::Arc::new(target);
            #adapter_call
        }
    })
}
