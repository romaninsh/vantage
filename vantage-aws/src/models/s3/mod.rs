//! Ready-made S3 tables — buckets, objects.
//!
//! S3 speaks REST-XML (see [`crate::restxml`]). Listing is shallow:
//! `ListBuckets` returns one row per bucket with name + creation
//! timestamp; `ListObjectsV2` returns one row per object key under a
//! given bucket. v0 doesn't paginate — anything past the first page
//! quietly drops, same caveat as the rest of `vantage-aws`.
//!
//! Path-style addressing only — `https://s3.{region}.amazonaws.com/{bucket}/`.
//! Buckets in regions other than the configured one will surface as
//! 301 redirects we don't follow; the caller is expected to point
//! `AwsAccount` at the bucket's home region first.

pub mod bucket;
pub mod object;

pub use bucket::{Bucket, buckets_table};
pub use object::{Object, objects_table};
