/// Implements a type system through a macro
///
/// vantage_type_system! {
///     type_trait: Type3,
///     method_name: my_value,
///     value_type: MyValueType,
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
            #[derive(Clone, Debug)]
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

                pub fn new_ref<T: $trait_name>(value: &T) -> Self {
                    Self {
                        value: value.[<to_ $method_name>](),
                        type_variant: Some(T::Target::TYPE_ENUM),
                    }
                }

                #[allow(clippy::ptr_arg)]
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

                pub fn into_value(self) -> $value_type {
                    self.value
                }
            }

            // Into/From trait implementations for AnyType ↔ ValueType
            impl From<[<Any $trait_name>]> for $value_type {
                fn from(any_value: [<Any $trait_name>]) -> Self {
                    any_value.into_value()
                }
            }

            impl TryFrom<$value_type> for [<Any $trait_name>] {
                type Error = vantage_core::VantageError;

                fn try_from(value: $value_type) -> vantage_core::Result<Self> {
                    Self::[<from_ $method_name>](&value).ok_or_else(|| vantage_core::error!("Failed to convert value to type"))
                }
            }

            // Persistence trait for structs
            pub trait [<$trait_name Persistence>]: Sized {
                fn [<to_ $trait_name:lower _map>](&self) -> indexmap::IndexMap<String, [<Any $trait_name>]>;
                fn [<from_ $trait_name:lower _map>](map: indexmap::IndexMap<String, [<Any $trait_name>]>) -> vantage_core::Result<Self>;

                // Convenient conversion methods that don't conflict with orphan rules
                fn [<to_ $trait_name:lower _record>](&self) -> vantage_types::Record<[<Any $trait_name>]> {
                    self.[<to_ $trait_name:lower _map>]().into()
                }

                fn [<to_ $trait_name:lower _values>](&self) -> vantage_types::Record<$value_type> {
                    self.[<to_ $trait_name:lower _map>]()
                        .into_iter()
                        .map(|(k, v)| (k, v.into_value()))
                        .collect()
                }

                fn [<from_ $trait_name:lower _record>](map: vantage_types::Record<[<Any $trait_name>]>) -> vantage_core::Result<Self> {
                    Self::[<from_ $trait_name:lower _map>](map.into_inner())
                }
            }




        }
    };
}
