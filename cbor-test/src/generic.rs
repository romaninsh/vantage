macro_rules! field_type_system {
    (
        type_trait: $trait_name:ident,
        method_name: $method:ident,
        value_type: $value_type:ty,
    ) => {
        paste::paste! {
            // Generate the main trait
            pub trait $trait_name: Send + Sync + std::fmt::Debug + Clone + 'static {
                fn [<to_ $method>](&self) -> $value_type;
                fn [<from_ $method>]($method: &$value_type) -> Option<Self>;
            }

            // Generate the type-erased trait
            trait [<$trait_name Erased>]: Send + Sync + std::fmt::Debug {
                fn [<to_ $method>](&self) -> $value_type;
                fn clone_boxed(&self) -> Box<dyn [<$trait_name Erased>]>;
                fn as_any(&self) -> &dyn std::any::Any;
            }

            // Blanket impl for the erased trait
            impl<T: $trait_name> [<$trait_name Erased>] for T {
                fn [<to_ $method>](&self) -> $value_type {
                    <Self as $trait_name>::[<to_ $method>](self)
                }

                fn clone_boxed(&self) -> Box<dyn [<$trait_name Erased>]> {
                    Box::new(self.clone())
                }

                fn as_any(&self) -> &dyn std::any::Any {
                    self
                }
            }

            // Generate the Any type-erased container
            #[derive(Debug)]
            pub struct [<Any $trait_name>] {
                value: Box<dyn [<$trait_name Erased>]>,
                type_id: std::any::TypeId,
            }

            impl [<Any $trait_name>] {
                pub fn new<T: $trait_name>(value: T) -> Self {
                    Self {
                        value: Box::new(value),
                        type_id: std::any::TypeId::of::<T>(),
                    }
                }

                pub fn [<to_ $method>](&self) -> $value_type {
                    self.value.[<to_ $method>]()
                }

                pub fn downcast_ref<T: $trait_name>(&self) -> Option<&T> {
                    if self.type_id == std::any::TypeId::of::<T>() {
                        self.value.as_any().downcast_ref::<T>()
                    } else {
                        None
                    }
                }

                pub fn [<from_ $method>]<T: $trait_name>($method: &$value_type) -> Option<Self> {
                    let value = T::[<from_ $method>]($method)?;
                    Some([<Any $trait_name>]::new(value))
                }
            }

            impl Clone for [<Any $trait_name>] {
                fn clone(&self) -> Self {
                    Self {
                        value: self.value.clone_boxed(),
                        type_id: self.type_id,
                    }
                }
            }
        }
    };
}

pub(crate) use field_type_system;
