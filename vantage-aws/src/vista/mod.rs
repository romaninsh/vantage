//! Vista bridge for the AWS backend.
//!
//! Construct a `Vista` from a typed `Table<AwsAccount, E>` via
//! `AwsAccount::vista_factory().from_table(...)`. AWS is read-only at
//! this stage — the shell advertises only `can_count`. YAML-driven
//! construction is stubbed; see [`factory::AwsVistaFactory`].

pub mod factory;
pub mod source;
pub mod spec;

pub use factory::AwsVistaFactory;
pub use source::AwsTableShell;
pub use spec::{AwsColumnExtras, AwsTableExtras, AwsVistaSpec};

use crate::AwsAccount;

impl AwsAccount {
    /// Return a Vista factory bound to this AWS account.
    pub fn vista_factory(&self) -> AwsVistaFactory {
        AwsVistaFactory::new(self.clone())
    }
}
