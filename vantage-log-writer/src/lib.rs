//! Append-only persistence that writes JSONL records to files.
//!
//! Insert-only: reads and expression execution return `Unsupported`. See
//! `docs4/src/new-persistence/` for the guide this crate illustrates.

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
