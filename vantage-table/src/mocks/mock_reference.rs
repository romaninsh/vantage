//! A relation that only describes itself, for tests of relation metadata.

use std::any::Any;

use vantage_core::Result;
use vantage_types::EmptyEntity;

use crate::mocks::mock_table_source::MockTableSource;
use crate::references::{Cardinality, Reference};
use crate::table::Table;

/// The table name of every [`MockReference`] target.
const TARGET_TABLE: &str = "target_table";

/// A [`Reference`] with a cardinality and a foreign key whose target is
/// named `"target_table"`. Over a [`MockTableSource`] its erased target is an
/// empty table of that name; anything else (another source, the join
/// columns, a traversal) panics. Lets metadata be tested on a source whose
/// values can't carry real relations.
#[derive(Debug, Clone)]
pub struct MockReference {
    cardinality: Cardinality,
    foreign_key: String,
}

impl MockReference {
    pub fn new(cardinality: Cardinality, foreign_key: impl Into<String>) -> Self {
        Self {
            cardinality,
            foreign_key: foreign_key.into(),
        }
    }
}

impl Reference for MockReference {
    fn columns(&self, _: &str, _: &str) -> (String, String) {
        unimplemented!("MockReference has no join columns")
    }

    fn foreign_key(&self) -> &str {
        &self.foreign_key
    }

    fn build_target(&self, _: &dyn Any) -> Box<dyn Any> {
        unimplemented!("MockReference has no target")
    }

    fn build_target_erased(&self, source: &dyn Any) -> Box<dyn Any> {
        let source = source
            .downcast_ref::<MockTableSource>()
            .expect("MockReference builds targets over a MockTableSource only");
        Box::new(Table::<MockTableSource, EmptyEntity>::new(
            TARGET_TABLE,
            source.clone(),
        ))
    }

    fn cardinality(&self) -> Cardinality {
        self.cardinality
    }

    fn resolve_from_row(&self, _: &dyn Any, _: &str, _: &dyn Any) -> Result<Box<dyn Any>> {
        unimplemented!("MockReference doesn't traverse")
    }

    fn target_table_name(&self, _: &dyn Any) -> String {
        TARGET_TABLE.to_string()
    }
}
