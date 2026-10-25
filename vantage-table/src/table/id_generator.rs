//! Auto-generation of record ids on insert.
//!
//! Some backends supply a primary key themselves (a SQL `DEFAULT`/sequence, an
//! in-memory store's own uuid). When the backend does not — a bare
//! `TEXT PRIMARY KEY`, a REST resource that expects the client to mint the id —
//! [`Table::with_generated_id`] fills the id column on insert using the existing
//! before-write hook machinery (see [`crate::table::hooks`]).
//!
//! It fills the id only when the inserted record carries none (the field is
//! absent or null), and only on insert: `patch`/`replace`/`update`/`upsert`
//! never touch the id. Because a present id is always kept, an id the caller
//! assigned once survives a retried insert — generation stays idempotent.
//!
//! [`IdGenerator`] itself lives in vantage-vista, so a type-erased `Vista`
//! (and every writer over one) mints ids the same way. A failing
//! [`IdGenerator::Fallible`] fails the insert.

pub use vantage_vista::{IdGenerator, ShortId};

use vantage_types::{Entity, InvariantValue};

use crate::table::{BeforeCx, BeforeFn, Hook, Phase, Table, before_hook};
use crate::traits::table_source::TableSource;

impl<T: TableSource, E: Entity<T::Value>> Table<T, E>
where
    T::Value: InvariantValue + From<String>,
{
    /// Fill the id column with a generated value whenever a new row is inserted
    /// without one.
    ///
    /// Only insert is affected, and only when the record's id field is absent or
    /// null — a caller-supplied id is always kept, so a retried insert that
    /// carries the same id stays idempotent. `patch`/`replace`/`update`/`upsert`
    /// never touch the id. Backends that mint their own id (e.g. the in-memory
    /// store, a SQL column with a `DEFAULT`) ignore the filled value and need no
    /// generator.
    pub fn with_generated_id(self, generator: IdGenerator) -> Self {
        self.with_hook(Hook::BeforeInsert(
            Phase::Populate,
            id_fill_hook::<T>(generator),
        ))
    }

    /// Mutable form of [`Self::with_generated_id`].
    pub fn add_generated_id(&mut self, generator: IdGenerator) {
        self.add_hook(Hook::BeforeInsert(
            Phase::Populate,
            id_fill_hook::<T>(generator),
        ));
    }
}

/// Build the before-insert hook that fills an absent/null id from `generator`.
/// The id column name is resolved from the table at fire time (falling back to
/// `"id"`), so the column need not exist when the generator is registered.
fn id_fill_hook<T: TableSource>(generator: IdGenerator) -> BeforeFn<T>
where
    T::Value: InvariantValue + From<String>,
{
    before_hook(move |cx: BeforeCx<'_, T>| {
        let id_field = cx.table.id_field_name();
        let generator = generator.clone();
        Box::pin(async move {
            let absent_or_null = match cx.record.get(id_field.as_str()) {
                None => true,
                Some(v) => v.is_null(),
            };
            if absent_or_null {
                cx.record
                    .insert(id_field, T::Value::from(generator.try_generate()?));
            }
            Ok(())
        })
    })
}
