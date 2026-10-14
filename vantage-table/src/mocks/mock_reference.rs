//! A relation that only describes itself, for tests of relation metadata.

use std::any::Any;

use vantage_core::Result;
use vantage_types::EmptyEntity;
use vantage_vista::{JoinKey, Reference as VistaReference};

use crate::mocks::mock_table_source::MockTableSource;
use crate::references::Reference;
use crate::table::Table;

/// The table name of every [`MockReference`] target.
const TARGET_TABLE: &str = "target_table";

#[derive(Debug, Clone, Copy)]
enum Join {
    One,
    Many,
}

/// A [`Reference`] with a join kind and a foreign key whose target is named
/// `"target_table"`. Over a [`MockTableSource`] its erased target is an
/// empty table of that name; anything else (another source, the join
/// columns, a traversal) panics. Lets metadata be tested on a source whose
/// values can't carry real relations.
#[derive(Debug, Clone)]
pub struct MockReference {
    join: Join,
    foreign_key: String,
}

impl MockReference {
    /// A has-one whose `foreign_key` lives on the source.
    pub fn has_one(foreign_key: impl Into<String>) -> Self {
        Self {
            join: Join::One,
            foreign_key: foreign_key.into(),
        }
    }

    /// A has-many whose `foreign_key` lives on the target.
    pub fn has_many(foreign_key: impl Into<String>) -> Self {
        Self {
            join: Join::Many,
            foreign_key: foreign_key.into(),
        }
    }
}

impl Reference for MockReference {
    fn columns(&self, _: &str, _: &str) -> (String, String) {
        unimplemented!("MockReference has no join columns")
    }

    fn vista_reference(&self, name: &str, target: String, source_id: &str) -> VistaReference {
        match self.join {
            Join::One => {
                VistaReference::has_one(name, target, JoinKey::to_target_id(&self.foreign_key))
            }
            Join::Many => {
                VistaReference::has_many(name, target, JoinKey::new(source_id, &self.foreign_key))
            }
        }
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

    fn resolve_from_row(&self, _: &dyn Any, _: &str, _: &dyn Any) -> Result<Box<dyn Any>> {
        unimplemented!("MockReference doesn't traverse")
    }

    fn target_table_name(&self, _: &dyn Any) -> String {
        TARGET_TABLE.to_string()
    }
}
