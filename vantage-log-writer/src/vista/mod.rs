//! Vista bridge for the log-writer backend.
//!
//! Insert-only: `can_insert: true`, all other capability flags false. Read
//! methods return `ErrorKind::Unsupported` via the trait's `default_error`.

pub mod factory;
pub mod source;
pub mod spec;

pub use factory::LogWriterVistaFactory;
pub use source::LogWriterTableShell;
pub use spec::{LogWriterBlock, LogWriterTableExtras, LogWriterVistaSpec};

use crate::log_writer::LogWriter;

impl LogWriter {
    pub fn vista_factory(&self) -> LogWriterVistaFactory {
        LogWriterVistaFactory::new(self.clone())
    }
}
