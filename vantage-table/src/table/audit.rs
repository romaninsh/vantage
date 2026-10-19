//! Auto-stamping of audit timestamps on write.
//!
//! [`Table::with_timestamps`] fills `created_at`/`updated_at` columns from the
//! wall clock using the before-write hook machinery (see [`crate::table::hooks`]):
//! `created_at` once, when a row is first inserted and only if the caller left it
//! empty; `updated_at` on every insert and update. Values are RFC 3339 UTC
//! strings (e.g. `2026-06-25T12:00:00Z`).
//!
//! Like [`crate::table::id_generator`], it leans on the same `From<String>`
//! value bound, so it works against any backend whose value type can carry a
//! string — the column itself can be plain `TEXT`.

use vantage_types::{Entity, InvariantValue};

use crate::table::{BeforeCx, BeforeFn, Hook, Phase, Table, before_hook};
use crate::traits::table_source::TableSource;

/// Which columns the audit stamps write. Defaults to `created_at` / `updated_at`.
#[derive(Clone, Debug)]
pub struct Timestamps {
    created_column: String,
    updated_column: String,
}

impl Default for Timestamps {
    fn default() -> Self {
        Self {
            created_column: "created_at".to_string(),
            updated_column: "updated_at".to_string(),
        }
    }
}

impl Timestamps {
    /// The default `created_at` / `updated_at` pair.
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the created-timestamp column name.
    pub fn created(mut self, column: impl Into<String>) -> Self {
        self.created_column = column.into();
        self
    }

    /// Override the updated-timestamp column name.
    pub fn updated(mut self, column: impl Into<String>) -> Self {
        self.updated_column = column.into();
        self
    }
}

impl<T: TableSource, E: Entity<T::Value>> Table<T, E>
where
    T::Value: InvariantValue + From<String>,
{
    /// Stamp `created_at` on insert and `updated_at` on every write, using the
    /// default column names.
    ///
    /// `created_at` is set only when the inserted record carries none (a
    /// caller-supplied value is kept, so a backfilled import keeps its real
    /// timestamps); `updated_at` is always (re)written. Both are RFC 3339 UTC
    /// strings. The columns need only be nullable `TEXT`.
    pub fn with_timestamps(self) -> Self {
        self.with_audit(Timestamps::default())
    }

    /// [`Self::with_timestamps`] with custom column names.
    pub fn with_audit(self, columns: Timestamps) -> Self {
        self.with_hook(Hook::BeforeInsert(
            Phase::Populate,
            created_hook::<T>(columns.created_column),
        ))
        .with_hook(Hook::BeforeSave(
            Phase::Populate,
            updated_hook::<T>(columns.updated_column),
        ))
    }
}

/// Before-insert hook: set `column` to the current time only if it is absent or
/// null (an explicit value survives).
fn created_hook<T: TableSource>(column: String) -> BeforeFn<T>
where
    T::Value: InvariantValue + From<String>,
{
    before_hook(move |cx: BeforeCx<'_, T>| {
        let column = column.clone();
        Box::pin(async move {
            let absent_or_null = match cx.record.get(column.as_str()) {
                None => true,
                Some(v) => v.is_null(),
            };
            if absent_or_null {
                cx.record.insert(column, T::Value::from(now_rfc3339()));
            }
            Ok(())
        })
    })
}

/// Before-save hook (insert + update): always set `column` to the current time.
fn updated_hook<T: TableSource>(column: String) -> BeforeFn<T>
where
    T::Value: InvariantValue + From<String>,
{
    before_hook(move |cx: BeforeCx<'_, T>| {
        let column = column.clone();
        Box::pin(async move {
            cx.record.insert(column, T::Value::from(now_rfc3339()));
            Ok(())
        })
    })
}

/// Current UTC time as a second-precision RFC 3339 string (`…Z`).
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
