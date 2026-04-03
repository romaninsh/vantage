use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::Parse, parse::ParseStream, parse_macro_input, punctuated::Punctuated, Data, DeriveInput,
    Fields, GenericArgument, Ident, PathArguments, Token, Type,
};

fn is_option_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            return segment.ident == "Option";
        }
    }
    false
}

fn extract_option_inner(ty: &Type) -> Option<&Type> {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Option" {
                if let PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(GenericArgument::Type(inner)) = args.args.first() {
                        return Some(inner);
                    }
                }
            }
        }
    }
    None
}

struct EntityArgs {
    type_names: Vec<Ident>,
}

impl Parse for EntityArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let punctuated: Punctuated<Ident, Token![,]> = Punctuated::parse_terminated(input)?;
        let mut seen = std::collections::HashSet::new();
        let mut type_names = Vec::new();
        for ident in punctuated {
            if !seen.insert(ident.to_string()) {
                return Err(syn::Error::new(
                    ident.span(),
                    format!("duplicate type system `{}`", ident),
                ));
            }
            type_names.push(ident);
        }
        if type_names.is_empty() {
            return Err(input.error("expected at least one type system name"));
        }
        Ok(EntityArgs { type_names })
    }
}

fn generate_impls(
    name: &Ident,
    entity_type: &Ident,
    fields: &syn::punctuated::Punctuated<syn::Field, Token![,]>,
) -> proc_macro2::TokenStream {
    let any_type = quote::format_ident!("Any{}", entity_type);

    let field_insertions = fields.iter().map(|field| {
        let field_name = field.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();
        quote! {
            map.insert(#field_name_str.to_string(), #any_type::new(self.#field_name.clone()));
        }
    });

    let field_extractions = fields.iter().map(|field| {
        let field_name = field.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();
        let field_type = &field.ty;

        if is_option_type(field_type) {
            let inner_type = extract_option_inner(field_type).unwrap();
            quote! {
                #field_name: match record.get(#field_name_str) {
                    Some(v) => v.try_get::<#field_type>()
                        .unwrap_or_else(|| v.try_get::<#inner_type>().map(Some).unwrap_or(None)),
                    None => None,
                }
            }
        } else {
            quote! {
                #field_name: record.get(#field_name_str)
                    .ok_or_else(|| vantage_core::error!("Missing field", field = #field_name_str))?
                    .try_get::<#field_type>()
                    .ok_or_else(|| vantage_core::error!("Failed to convert field", field = #field_name_str))?
            }
        }
    });

    quote! {
        impl vantage_types::IntoRecord<#any_type> for #name {
            fn into_record(self) -> vantage_types::Record<#any_type> {
                let mut map = indexmap::IndexMap::new();
                #(#field_insertions)*
                map.into()
            }
        }

        impl vantage_types::TryFromRecord<#any_type> for #name {
            type Error = vantage_core::VantageError;

            fn from_record(record: vantage_types::Record<#any_type>) -> vantage_core::Result<Self> {
                Ok(Self {
                    #(#field_extractions),*
                })
            }
        }
    }
}

#[proc_macro_attribute]
pub fn entity(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as EntityArgs);
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let fields = match &input.data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("Entity only supports structs with named fields"),
        },
        _ => panic!("Entity only supports structs"),
    };

    let all_impls: Vec<_> = args
        .type_names
        .iter()
        .map(|t| generate_impls(name, t, fields))
        .collect();

    let expanded = quote! {
        #input

        #(#all_impls)*
    };

    TokenStream::from(expanded)
}
