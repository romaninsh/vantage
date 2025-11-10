/// Implements a type system through a macro
///
/// vantage_type_system! {
///     type_trait: Type3,
///     method_name: cbor,
///     value_type: ciborium::value::Value,
///     type_variants: [String, Email]
/// }
#[macro_export]
macro_rules! vantage_type_system {
    (
        type_trait: $trait_name:ident,
        method_name: $method_name:ident,
        value_type: $value_type:ty,
        type_variants: [$($variant:ident),* $(,)?]
    ) => {
        // Generate enum for type variants
        paste::paste! {
            #[derive(Debug, PartialEq, Copy, Clone)]
            pub enum [<$trait_name Variants>] {
                $($variant,)*
            }

            // Generate marker trait
            pub trait [<$trait_name Marker>] {
                const TYPE_ENUM: [<$trait_name Variants>];
            }

            // Generate marker structs for each variant
            $(
                pub struct [<$trait_name $variant Marker>];
                impl [<$trait_name Marker>] for [<$trait_name $variant Marker>] {
                    const TYPE_ENUM: [<$trait_name Variants>] = [<$trait_name Variants>]::$variant;
                }
            )*

            // Main trait
            pub trait $trait_name: 'static + Sized {
                type Target: [<$trait_name Marker>];

                fn [<to_ $method_name>](&self) -> $value_type;
                fn [<from_ $method_name>](value: $value_type) -> Option<Self>;
            }

            // Any type wrapper
            #[derive(Clone)]
            pub struct [<Any $trait_name>] {
                value: $value_type,
                type_variant: Option<[<$trait_name Variants>]>,
            }

            impl [<Any $trait_name>] {
                pub fn new<T: $trait_name>(value: T) -> Self {
                    Self {
                        value: value.[<to_ $method_name>](),
                        type_variant: Some(T::Target::TYPE_ENUM),
                    }
                }

                pub fn [<from_ $method_name>](value: &$value_type) -> Option<Self> {
                    let type_variant = [<$trait_name Variants>]::[<from_ $method_name>](value);
                    let value = value.clone();
                    Some(Self {
                        value,
                        type_variant,
                    })
                }

                pub fn value(&self) -> &$value_type {
                    &self.value
                }

                pub fn type_variant(&self) -> Option<[<$trait_name Variants>]> {
                    self.type_variant
                }

                pub fn try_get<T: $trait_name>(&self) -> Option<T> {
                    if self.type_variant.is_none() || self.type_variant == Some(T::Target::TYPE_ENUM) {
                        T::[<from_ $method_name>](self.value.clone())
                    } else {
                        None
                    }
                }
            }

            // Persistence trait for structs
            pub trait [<$trait_name Persistence>]: Sized {
                fn [<to_ $trait_name:lower _map>](&self) -> indexmap::IndexMap<String, [<Any $trait_name>]>;
                fn [<from_ $trait_name:lower _map>](map: indexmap::IndexMap<String, [<Any $trait_name>]>) -> Option<Self>;
            }

            // Helper functions similar to cbor.rs
            pub fn [<to_ $method_name _value>]<T: serde::Serialize>(value: &T) -> $value_type {
                [<to_ $method_name _value_result>](value).unwrap()
            }

            pub fn [<to_ $method_name _value_result>]<T: serde::Serialize>(
                value: &T,
            ) -> Result<$value_type, ciborium::ser::Error<std::io::Error>> {
                let mut buffer = Vec::new();
                ciborium::ser::into_writer(value, &mut buffer)?;
                let cbor_value: $value_type = ciborium::de::from_reader(&buffer[..]).map_err(|_| {
                    ciborium::ser::Error::Io(std::io::Error::from(std::io::ErrorKind::InvalidData))
                })?;
                Ok(cbor_value)
            }

            pub fn [<from_ $method_name _value>]<T: for<'de> serde::Deserialize<'de>>(value: $value_type) -> T {
                [<from_ $method_name _value_result>](value).unwrap()
            }

            pub fn [<from_ $method_name _value_result>]<T: for<'de> serde::Deserialize<'de>>(
                value: $value_type,
            ) -> Result<T, ciborium::de::Error<std::io::Error>> {
                let mut buffer = Vec::new();
                ciborium::ser::into_writer(&value, &mut buffer).map_err(|_| {
                    ciborium::de::Error::Io(std::io::Error::from(std::io::ErrorKind::InvalidData))
                })?;
                ciborium::de::from_reader(&buffer[..])
            }
        }
    };
}

// Example usage:
// vantage_type_system! {
//     type_trait: Type3,
//     method_name: cbor,
//     value_type: ciborium::Value,
//     type_variants: [String, Email, Number]
// }
