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
    let type_lower = persistence_type.to_string().to_lowercase();

    let fields = match &input.data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("Persistence only supports structs with named fields"),
        },
        _ => panic!("Persistence only supports structs"),
    };

    let any_type = quote::format_ident!("Any{}", persistence_type);
    let trait_name = quote::format_ident!("{}Persistence", persistence_type);
    let to_method = quote::format_ident!("to_{}_map", type_lower);
    let from_method = quote::format_ident!("from_{}_map", type_lower);

    let field_insertions = fields.iter().map(|field| {
        let field_name = field.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();
        quote! {
            map.insert(#field_name_str.to_string(), crate::#any_type::new(self.#field_name.clone()));
        }
    });

    let field_extractions = fields.iter().map(|field| {
        let field_name = field.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();
        let field_type = &field.ty;
        quote! {
            #field_name: map.get(#field_name_str)?.try_get::<#field_type>()?
        }
    });

    let expanded = quote! {
        #input

        impl #trait_name for #name {
            fn #to_method(&self) -> indexmap::IndexMap<String, crate::#any_type> {
                let mut map = indexmap::IndexMap::new();
                #(#field_insertions)*
                map
            }

            fn #from_method(map: indexmap::IndexMap<String, crate::#any_type>) -> Option<Self> {
                Some(Self {
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
