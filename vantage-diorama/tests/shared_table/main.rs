//! Dios over one table on one Lens see each other's settled changes.
//!
//! A Dio derived for "client 7" keeps its own cache table and bus, so a row
//! its parent moves to client 8 used to stay listed there, stale, forever.
//! Through the Lens's table hub, every Dio over the table now publishes what
//! settles in its cache and judges what the others publish against its own
//! query: a row that leaves its set leaves its views, one that enters shows
//! up, and one inside it repaints.

mod fixtures;
mod isolation;
mod live;
mod recheck;
mod reconcile;
#[path = "../support/mod.rs"]
mod support;
mod two_pass;
mod writes;
