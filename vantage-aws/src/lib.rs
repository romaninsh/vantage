//! AWS API wrapper for Vantage — incubating.
//!
//! Treat AWS API endpoints as Vantage `TableSource`s. Build an
//! [`AwsAccount`], hand it to a `Table`, and encode the operation in
//! the table name as `{protocol}/{array_key}:{service}/{target}`:
//!
//! ```text
//! json1/logGroups:logs/Logs_20140328.DescribeLogGroups
//!   │       │      │    └── X-Amz-Target header value
//!   │       │      └─────── service code (also URL hostname segment)
//!   │       └────────────── response field that holds the row array
//!   └────────────────────── wire protocol
//!
//! query/Users:iam/2010-05-08.ListUsers
//!   │     │    │     │
//!   │     │    │     └── "VERSION.Action" — both go in the form body
//!   │     │    └──────── service code (also URL hostname segment)
//!   │     └───────────── response element name (Query lists wrap in <member>)
//!   └─────────────────── wire protocol
//! ```
//!
//! Two protocols ship today: **JSON-1.1** for CloudWatch, ECS, KMS,
//! DynamoDB control-plane, etc.; and **Query** (form-encoded request,
//! XML response) for IAM, STS, EC2, ELBv1, SES, etc. Both are routed
//! through the same `AwsAccount` `TableSource`, so relations span
//! services and protocols freely.
//!
//! Conditions on the table fold into the request body. v0 is
//! read-only, first-page only — pagination and writes arrive later.
//!
//! Ready-made models live under [`models`] if you want to skip the
//! table-name dance and start querying.
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
//!     "json1/logGroups:logs/Logs_20140328.DescribeLogGroups",
//!     aws,
//! );
//! groups.add_condition(eq("logGroupNamePrefix", "/aws/lambda/"));
//! # Ok(()) }
//! ```

mod account;
mod condition;
mod dispatch;
mod impls;
mod json1;
mod json10;
mod operation;
mod query;
mod restjson;
mod restxml;
mod sign;

pub mod models;
pub mod types;

pub use account::AwsAccount;
pub use condition::{AwsCondition, eq, in_};
pub use operation::AwsOperation;
pub use types::{AnyAwsType, Arn, AwsDateTime, typed_records, untyped_records};

#[doc(hidden)]
pub mod __test_support {
    //! Internal hooks for integration tests under `tests/`. Not part of
    //! the public API.
    pub use crate::sign::{SignedHeader, sign_v4};
}
