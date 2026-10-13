//! Every clause a read path's master doesn't take is evaluated over a
//! complete set or fails loudly: one test per kind of residual.
//!
//! - an eager view evaluates it over the cache, which holds the whole set;
//! - a paged view with no eager start fails, naming the table and the
//!   clause, and shows no rows;
//! - `Dio::fetch` hands it back in `refused`;
//! - the facade evaluates it over the rows it reads.

#[path = "../support/mod.rs"]
mod support;

mod augmented_failures;
mod augmented_heal;
mod augmented_paths;
mod augmented_reads;
mod augmented_views;
mod big_bucket;
mod conditions;
mod dio_order;
mod drivers;
mod facade;
mod fixtures;
mod picker_listing;
mod pickers;
mod races;
mod reads;
mod rejected;
mod reread;
mod search_order;
