//! DynamoDB persistence — incubating.
//!
//! Sibling to the AWS list-API surface in `crate::models`. That surface
//! treats `AwsAccount` itself as a `TableSource` and folds operation
//! metadata into the table name. DynamoDB items don't fit that mould:
//! they have a typed `AttributeValue` representation, native key/filter
//! expressions, and full CRUD semantics. So DynamoDB lives here as a
//! standalone persistence — same crate, different `TableSource`.
//!
//! Layout mirrors `vantage-sql`'s per-backend modules: `types/` defines
//! the type system, `condition.rs` the native condition DSL,
//! `operation.rs` the typed operator extension trait, and `impls/`
//! holds the trait implementations.
//!
//! ```no_run
//! # use vantage_aws::AwsAccount;
//! # use vantage_aws::dynamodb::DynamoDB;
//! # async fn run() -> vantage_core::Result<()> {
//! let aws = AwsAccount::from_default()?;
//! let _db = DynamoDB::new(aws);
//! # Ok(()) }
//! ```

pub mod condition;
pub mod id;
pub mod impls;
pub mod operation;
pub mod types;

pub(crate) mod transport;
pub(crate) mod wire;

pub use condition::DynamoCondition;
pub use id::DynamoId;
pub use operation::DynamoOperation;
pub use types::{AnyDynamoType, AttributeValue, DynamoType, DynamoTypeVariants};

use crate::account::AwsAccount;

/// DynamoDB data source. Cheap to clone — wraps the (already `Arc`-backed)
/// `AwsAccount` for credentials and signing.
#[derive(Clone, Debug)]
pub struct DynamoDB {
    aws: AwsAccount,
}

impl DynamoDB {
    /// Build a DynamoDB handle on top of an existing `AwsAccount`.
    pub fn new(aws: AwsAccount) -> Self {
        Self { aws }
    }

    /// Borrow the underlying `AwsAccount` (for signing, region, etc.).
    pub fn aws(&self) -> &AwsAccount {
        &self.aws
    }
}
