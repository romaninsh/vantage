//! [`FakerColumn`]: one column's name, declared type and optional explicit
//! generator.

use crate::generator::ColumnGen;

/// One column of a faker table: a name, a declared type and an optional
/// explicit generator. [`ValueGen`](crate::ValueGen) uses `generator` if set,
/// else `name`, then `ty`, to pick a value.
#[derive(Clone, Debug, Default)]
pub struct FakerColumn {
    pub name: String,
    pub ty: String,
    pub generator: Option<ColumnGen>,
}

impl FakerColumn {
    /// A column with no generator.
    pub fn new(name: impl Into<String>, ty: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ty: ty.into(),
            generator: None,
        }
    }

    /// Generate this column's values with `generator`.
    pub fn with_generator(mut self, generator: ColumnGen) -> Self {
        self.generator = Some(generator);
        self
    }
}
