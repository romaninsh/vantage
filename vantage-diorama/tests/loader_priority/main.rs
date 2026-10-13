//! The loader, not the lens and not the transport, decides who is waiting:
//! a viewport nobody has cached is essential, a poll over cached rows is
//! background, and the priority reaches the paged loader through the
//! task-local even though the loader runs on its own task (`priority`).
//!
//! The same binary covers what a load leaves behind when it does not finish:
//! a newer request cancels or parks behind a running one (`cancellation`), a
//! cancelled load restores the visible map to what it was rather than
//! deleting the rows it happened to overwrite (`restore`), and so does a
//! failed one (`failure`).

#[path = "../support/mod.rs"]
mod support;

mod cancellation;
mod failure;
mod fixtures;
mod priority;
mod restore;
