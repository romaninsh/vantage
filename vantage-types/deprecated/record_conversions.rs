//! Deprecated macros for record conversions
//!
//! These macros depended on the old IndexMap-based persistence methods
//! that were removed in favor of the simpler Record-based approach.
//!
//! This file is kept for reference but is not included in the main library.

use paste::paste;

/// Deprecated: Generate Into/TryFrom implementations for struct -> Record conversions
///
/// This macro used to generate conversions between structs and Record types
/// using the old *Persistence trait methods. It's no longer needed since
/// the #[persistence] macro now generates IntoRecord/TryFromRecord directly.
///
/// Use the IntoRecord/TryFromRecord traits directly instead:
/// ```
/// use vantage_types::{IntoRecord, TryFromRecord};
///
/// // Convert struct to record
/// let record: Record<AnyType> = my_struct.into_record();
///
/// // Convert record back to struct
/// let my_struct = MyStruct::from_record(record)?;
/// ```
#[macro_export]
macro_rules! vantage_record_conversions {
    ($struct_name:ty, $trait_name:ident, $value_type:ty) => {
        paste! {
            // Into implementations for specific struct
            impl Into<vantage_types::Record<[<Any $trait_name>]>> for $struct_name {
                fn into(self) -> vantage_types::Record<[<Any $trait_name>]> {
                    // This would call the old to_*_map() method that no longer exists
                    self.[<to_ $trait_name:lower _map>]().into()
                }
            }

            impl Into<vantage_types::Record<$value_type>> for $struct_name {
                fn into(self) -> vantage_types::Record<$value_type> {
                    // This would call the old to_*_values() method that no longer exists
                    self.[<to_ $trait_name:lower _values>]()
                }
            }

            // TryFrom for reverse conversion
            impl TryFrom<vantage_types::Record<[<Any $trait_name>]>> for $struct_name {
                type Error = vantage_core::VantageError;

                fn try_from(value: vantage_types::Record<[<Any $trait_name>]>) -> vantage_core::Result<Self> {
                    // This would call the old from_*_map() method that no longer exists
                    Self::[<from_ $trait_name:lower _map>](value.into_inner())
                }
            }
        }
    };
}
