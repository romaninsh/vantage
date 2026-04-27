//! AWS API wrapper for Vantage — incubating.
//!
//! Thin layer that lets you treat AWS JSON-1.1 RPC endpoints (CloudWatch
//! Logs, ECS, DynamoDB control plane, KMS, Lambda invoke, …) as Vantage
//! `TableSource`s. Each `Table::new("service/target", aws.json1(array_key))`
//! is one AWS operation; conditions on the table fold into the JSON
//! request body, the response array is parsed into `Record<Value>`.
//!
//! v0 scope: read-only, first page only, JSON 1.1 only. Lambda-style
//! REST-JSON arrives in a future `aws.rest_json(...)` source.
//!
//! ```no_run
//! # use vantage_aws::{AwsAccount, AwsJson1, eq};
//! # use vantage_table::table::Table;
//! # use vantage_types::EmptyEntity;
//! # async fn run() -> vantage_core::Result<()> {
//! let aws = AwsAccount::from_env()?;
//!
//! // List CloudWatch log groups.
//! let groups: Table<AwsJson1, EmptyEntity> = Table::new(
//!     "logs/Logs_20140328.DescribeLogGroups",
//!     aws.json1("logGroups"),
//! );
//!
//! // Filter by prefix — folded into the request body as `logGroupNamePrefix`.
//! let mut filtered = groups.clone();
//! filtered.add_condition(eq("logGroupNamePrefix", "/aws/lambda/"));
//! # Ok(()) }
//! ```

mod account;
mod condition;
mod json1;
mod operation;
mod sign;
mod transport;

pub use account::AwsAccount;
pub use condition::{AwsCondition, eq, in_};
pub use json1::AwsJson1;
pub use operation::AwsOperation;

#[doc(hidden)]
pub mod __test_support {
    //! Internal hooks for integration tests under `tests/`. Not part of
    //! the public API.
    pub use crate::sign::{SignedHeader, sign_v4};
}
