use proc_macro::TokenStream;
use quote::quote;
use syn::{parse::Parse, parse::ParseStream, parse_macro_input, Data, DeriveInput, Fields, Ident};

struct PersistenceArgs {
    type_name: Ident,
}

impl Parse for PersistenceArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let type_name: Ident = input.parse()?;
        Ok(PersistenceArgs { type_name })
    }
}

#[proc_macro_attribute]
pub fn persistence(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as PersistenceArgs);
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let persistence_type = &args.type_name;

    let fields = match &input.data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("Persistence only supports structs with named fields"),
        },
        _ => panic!("Persistence only supports structs"),
    };

    let any_type = quote::format_ident!("Any{}", persistence_type);

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
        quote! {
            #field_name: record.get(#field_name_str)
                .ok_or_else(|| vantage_core::error!("Missing field", field = #field_name_str))?
                .try_get::<#field_type>()
                .ok_or_else(|| vantage_core::error!("Failed to convert field", field = #field_name_str))?
        }
    });

    let expanded = quote! {
        #input

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

    };

    TokenStream::from(expanded)
}

#[cfg(feature = "serde")]
#[proc_macro_attribute]
pub fn persistence_serde(_args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let expanded = quote! {
        #input

        // Add Into implementation for Record<serde_json::Value>
        impl Into<vantage_types::Record<serde_json::Value>> for #name {
            fn into(self) -> vantage_types::Record<serde_json::Value> {
                vantage_types::Record::from_serializable(self).expect("Failed to serialize to JSON")
            }
        }

        // Add TryFrom implementation for reverse conversion
        impl TryFrom<vantage_types::Record<serde_json::Value>> for #name {
            type Error = serde_json::Error;

            fn try_from(record: vantage_types::Record<serde_json::Value>) -> Result<Self, Self::Error> {
                record.to_deserializable()
            }
        }
    };

    TokenStream::from(expanded)
}

#[cfg(not(feature = "serde"))]
#[proc_macro_attribute]
pub fn persistence_serde(_args: TokenStream, input: TokenStream) -> TokenStream {
    // When serde feature is disabled, just return the original struct
    input
}
