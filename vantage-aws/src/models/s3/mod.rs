//! Ready-made S3 tables — buckets, objects — plus a raw content fetch.
//!
//! S3 speaks REST-XML (see [`crate::restxml`]). Listing is shallow:
//! `ListBuckets` returns one row per bucket with name + creation
//! timestamp; `ListObjectsV2` returns one row per object key under a
//! given bucket, auto-paginating via its continuation-token cursor.
//!
//! Path-style addressing only — `https://s3.{region}.amazonaws.com/{bucket}/`.
//! Buckets in regions other than the configured one will surface as
//! 301 redirects we don't follow; the caller is expected to point
//! `AwsAccount` at the bucket's home region first.

pub mod bucket;
pub mod object;

pub use bucket::{Bucket, buckets_table};
pub use object::{Object, get_object, objects_table};
