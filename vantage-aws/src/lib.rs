//! AWS API wrapper for Vantage — incubating.
//!
//! Treat AWS JSON-1.1 RPC endpoints (CloudWatch Logs, ECS, DynamoDB
//! control plane, KMS, …) as Vantage `TableSource`s. Build an
//! [`AwsAccount`], hand it to a `Table`, and encode the operation in
//! the table name as `array_key:service/target`:
//!
//! ```text
//! "logGroups:logs/Logs_20140328.DescribeLogGroups"
//!     │       │       └── X-Amz-Target header value
//!     │       └────────── service code (also the URL hostname segment)
//!     └────────────────── response field that holds the row array
//! ```
//!
//! Conditions on the table fold into the JSON request body. v0 is
//! read-only, first-page only, JSON-1.1 only — REST-JSON and S3 will
//! arrive as separate source types.
//!
//! Ready-made CloudWatch models live under [`models`] if you want to
//! skip the table-name dance and start querying.
//!
//! ```no_run
//! # use vantage_aws::{AwsAccount, eq};
//! # use vantage_table::table::Table;
//! # use vantage_types::EmptyEntity;
//! # async fn run() -> vantage_core::Result<()> {
//! // env vars first, falling back to ~/.aws/credentials [default]
//! let aws = AwsAccount::from_default()?;
//!
//! let mut groups: Table<AwsAccount, EmptyEntity> = Table::new(
//!     "logGroups:logs/Logs_20140328.DescribeLogGroups",
//!     aws,
//! );
//! groups.add_condition(eq("logGroupNamePrefix", "/aws/lambda/"));
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
