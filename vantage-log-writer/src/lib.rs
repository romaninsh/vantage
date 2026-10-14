//! Append-only persistence that writes JSONL records to files.
//!
//! See `docs4/src/new-persistence/` for the full guide; this crate is the
//! POC implementation of step5 (insert-only TableSource). It has a type
//! system (`type_system`) and, behind the `vista` feature, a vista layer;
//! expressions are not executed.

mod log_writer;
mod table_source;
pub mod type_system;
#[cfg(feature = "vista")]
pub mod vista;
mod writer_task;

pub use log_writer::LogWriter;
pub use type_system::{AnyJsonType, JsonType, JsonTypeVariants};
#[cfg(feature = "vista")]
pub use vista::{LogWriterTableShell, LogWriterVistaFactory};
