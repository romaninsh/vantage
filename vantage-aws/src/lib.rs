//! AWS API wrapper for Vantage — incubating.
//!
//! Thin layer that lets you treat AWS JSON-1.1 RPC endpoints (CloudWatch
//! Logs, ECS, DynamoDB control plane, KMS, Lambda invoke, …) as Vantage
//! `TableSource`s. `AwsAccount` *is* the source — per-operation config
//! lives in the table name, formatted as `array_key:service/target`:
//!
//! ```text
//! "logGroups:logs/Logs_20140328.DescribeLogGroups"
//!     │       │       └── X-Amz-Target header value
//!     │       └────────── service code
//!     └────────────────── response field that holds the row array
//! ```
//!
//! Conditions on the table fold into the JSON request body; the
//! response array is parsed into `Record<CborValue>`.
//!
//! v0 scope: read-only, first page only, JSON 1.1 only. Lambda-style
//! REST-JSON and S3 arrive as separate source types in a future
//! revision.
//!
//! ```no_run
//! # use vantage_aws::{AwsAccount, eq};
//! # use vantage_table::table::Table;
//! # use vantage_types::EmptyEntity;
//! # async fn run() -> vantage_core::Result<()> {
//! // env vars first, falling back to ~/.aws/credentials [default]
//! let aws = AwsAccount::from_default()?;
//!
//! // List CloudWatch log groups.
//! let groups: Table<AwsAccount, EmptyEntity> = Table::new(
//!     "logGroups:logs/Logs_20140328.DescribeLogGroups",
//!     aws.clone(),
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

pub mod models;

pub use account::AwsAccount;
pub use condition::{AwsCondition, eq, in_};
pub use operation::AwsOperation;

#[doc(hidden)]
pub mod __test_support {
    //! Internal hooks for integration tests under `tests/`. Not part of
    //! the public API.
    pub use crate::sign::{SignedHeader, sign_v4};
}
