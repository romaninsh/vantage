//! Vista bridge for the log-writer backend.
//!
//! Insert-only: `Insert` is the one advertised capability, and every read
//! returns `ErrorKind::Unsupported`.

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
