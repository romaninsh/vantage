//! # vantage-live
//!
//! Live data synchronization layer for Vantage framework.
//!
//! Provides in-memory caching with async backend persistence, conflict detection,
//! and editing sessions for building responsive UIs.
//!
//! ## Key Features
//!
//! - **LiveTable**: Cache layer over any backend storage (SQL, SurrealDB, etc.)
//! - **RecordEdit**: Editing sessions with change tracking and snapshot management
//! - **Async persistence**: Write operations happen in background
//! - **Conflict detection**: Track remote changes vs local edits
//!
//! ## Example
//!
//! ```rust,ignore
//! use vantage_live::LiveTable;
//!
//! // Create LiveTable with backend and cache
//! let backend = SurrealTable::new(client, "bakery");
//! let cache = ImTable::new(&im_ds, "bakery_cache");
//! let mut live_table = LiveTable::new(backend, cache).await?;
//!
//! // Edit existing record
//! let mut edit = live_table.edit_record("bakery:123").await?;
//! edit.name = "New Name".to_string();
//!
//! // Check what changed
//! println!("Modified: {:?}", edit.get_modified_fields());
//!
//! // Save when ready
//! match edit.save().await? {
//!     SaveResult::Saved => println!("Success!"),
//!     SaveResult::Error(e) => eprintln!("Failed: {}", e),
//!     _ => {}
//! }
//! ```

mod helpers;
mod live_table;
mod record_edit;

pub use live_table::LiveTable;
pub use record_edit::{RecordEdit, SaveResult};

pub mod prelude {
    pub use crate::{LiveTable, RecordEdit, SaveResult};
}
