//! Write-queue worker — stage 3.
//!
//! Stage 1 leaves this module empty so the file exists in the layout
//! the architecture doc advertises. Stage 3 introduces the loop that
//! consumes `WriteOp`s, fires `on_write` (or the default cache+master
//! write), and reports failures onto the event bus.
