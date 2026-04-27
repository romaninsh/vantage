//! AWS resource models — proof-of-concept.
//!
//! Two CloudWatch resources, wired up to demonstrate that `vantage-aws`
//! can drive real AWS endpoints:
//!
//! - [`LogGroup`] — `Logs_20140328.DescribeLogGroups`
//! - [`LogEvent`] — `Logs_20140328.FilterLogEvents`
//!
//! Cross-resource navigation: [`LogGroup::ref_events`] returns a
//! pre-conditioned `Table<AwsAccount, LogEvent>`. AWS doesn't accept
//! multi-value filters, so traversal is one-parent-at-a-time;
//! hand-written `ref_*` methods on entities are the v0 idiom.

pub mod log_event;
pub mod log_group;

pub use log_event::{LogEvent, log_events_table};
pub use log_group::{LogGroup, log_groups_table};
