//! Facade writes: each is held to the handle's narrowing, then written
//! through [`Dio::flash`](crate::Dio::flash), which holds it to the Dio's own
//! set, stages it and waits for it to settle. A write returns once it is
//! confirmed, or with its error once it is rolled back.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;

use crate::dio::flash::confine::SetRule;
use crate::dio::shell::DioShell;
use crate::ops::{ChangeFlash, FlashOutcome};

impl DioShell {
    /// With an id, insert-if-absent: an id already in view returns the stored
    /// row and writes nothing; an id held outside the narrowing is `Conflict`.
    ///
    /// Without one, the backend makes the id
    /// ([`ChangeFlash::insert_new`]); the row is in the cache under it by the
    /// time this returns.
    pub(super) async fn insert_row(
        &self,
        id: Option<&str>,
        record: &Record<CborValue>,
    ) -> Result<(String, Record<CborValue>)> {
        let Some(id) = id else {
            let record = self.rule().conform(record)?;
            let outcome = self.dio().flash(ChangeFlash::insert_new(record)).await?;
            let id = outcome.id.clone().ok_or_else(|| {
                error!(
                    "the insert settled without reporting the new row's id",
                    table = self.dio.cache_table_name
                )
            })?;
            let stored = self.stored(outcome, &id, None);
            return Ok((id, stored));
        };
        if let Some(row) = self.row_in_view_or_conflict(id).await? {
            return Ok((id.to_string(), row));
        }
        let record = self.rule().conform(record)?;
        let outcome = self
            .dio()
            .flash(ChangeFlash::insert(id, record.clone()))
            .await?;
        let settled = outcome.id.clone().unwrap_or_else(|| id.to_string());
        let stored = self.stored(outcome, &settled, Some(&record));
        Ok((settled, stored))
    }

    pub(super) async fn replace_row(
        &self,
        id: &str,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.row_in_view_or_conflict(id).await?;
        let record = self.rule().conform(record)?;
        let outcome = self
            .dio()
            .flash(ChangeFlash::replace(id, record.clone()))
            .await?;
        Ok(self.stored(outcome, id, Some(&record)))
    }

    pub(super) async fn upsert_row(
        &self,
        id: &str,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.row_in_view_or_conflict(id).await?;
        let record = self.rule().conform(record)?;
        let outcome = self
            .dio()
            .flash(ChangeFlash::upsert(id, record.clone()))
            .await?;
        Ok(self.stored(outcome, id, Some(&record)))
    }

    /// A missing row, or one outside the narrowing, is `NotFound`.
    pub(super) async fn patch_row(
        &self,
        id: &str,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let Some(before) = self.row_in_view(id).await? else {
            return Err(not_found(self, id));
        };
        self.rule().check_patch(id, &before, partial)?;
        self.dio()
            .flash(ChangeFlash::partial(id, partial.clone()))
            .await?;
        Ok(self.with_injected_id(partial, id))
    }

    /// Idempotent: a row missing or outside the narrowing is already gone
    /// from this set, so nothing is written and the call succeeds.
    pub(super) async fn delete_row(&self, id: &str) -> Result<()> {
        if self.row_in_view(id).await?.is_some() {
            self.dio().flash(ChangeFlash::delete(id)).await?;
        }
        Ok(())
    }

    /// A handle whose rows are the whole cache sends one `Clear`, which the
    /// Dio confines to its own set. Otherwise each row it reads goes through
    /// [`delete_row`](Self::delete_row), so a stale cached row the master has
    /// moved out of the set is left alone. Read from the cache, the subset is
    /// the rows the cache holds; on a paged Dio that may be fewer than the
    /// master's.
    pub(super) async fn delete_rows(&self) -> Result<()> {
        if self.reads_whole_cache() {
            return self.dio().flash(ChangeFlash::clear()).await.map(|_| ());
        }
        for id in self.read().await?.into_keys() {
            self.delete_row(&id).await?;
        }
        Ok(())
    }

    /// The handle's narrowing as a write rule.
    fn rule(&self) -> SetRule<'_> {
        SetRule::new(&self.query, &self.dio)
    }

    fn dio(&self) -> crate::Dio {
        crate::Dio {
            inner: self.dio.clone(),
        }
    }

    /// The row with `id`, ignoring the narrowing — one point read, never a
    /// listing.
    ///
    /// - An unnarrowed handle has no set to protect: it reads the cache,
    ///   then the master on a miss, so a cache hit never touches the master.
    /// - A handle whose narrowing names an augmented value reads the cache
    ///   too, hydrated: only the cache ever holds that value. On a miss it
    ///   reads the master and runs the Dio's augmentations over that row in
    ///   memory, so the whole narrowing judges it.
    /// - Any other narrowed handle reads the master only, because a stale
    ///   cached row must not decide whether a write stays inside the set.
    async fn held(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        let augmented = self.rule().needs_hydration();
        if augmented
            && let Some(row) = self
                .dio
                .read_hydrated(self.dio.cache.as_ref(), id, &self.query)
                .await?
        {
            return Ok(Some(row));
        }
        if !self.narrows()
            && let Some(row) = self.dio.cache.get_value(id).await?
        {
            return Ok(Some(row));
        }
        match self.dio.master.get_value(id).await? {
            Some(row) if augmented => Ok(Some(self.dio.augment_in_memory(row).await?)),
            held => Ok(held),
        }
    }

    /// The row `id` as this handle sees it: `held`, kept only when it
    /// satisfies the narrowing.
    async fn row_in_view(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        match self.held(id).await? {
            Some(row) if self.rule().admits(&row)? => Ok(Some(row)),
            _ => Ok(None),
        }
    }

    /// Like `row_in_view`, but an id held by a row outside the narrowing is
    /// `Conflict`: a full-record write may create the row, never take over
    /// one outside the set.
    async fn row_in_view_or_conflict(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        let row = self.held(id).await?;
        self.rule().not_outside(id, row)
    }

    /// The record a confirmed write reports: the stored row when the write
    /// returned one, else `written` (or nothing) with `id` under the id
    /// column.
    fn stored(
        &self,
        outcome: FlashOutcome,
        id: &str,
        written: Option<&Record<CborValue>>,
    ) -> Record<CborValue> {
        outcome
            .record
            .unwrap_or_else(|| self.with_injected_id(written.unwrap_or(&Record::new()), id))
    }

    /// `record` with `id` under the Dio's id column.
    fn with_injected_id(&self, record: &Record<CborValue>, id: &str) -> Record<CborValue> {
        let mut out = record.clone();
        out.insert(self.dio.id_column.clone(), CborValue::Text(id.to_string()));
        out
    }
}

fn not_found(shell: &DioShell, id: &str) -> vantage_core::VantageError {
    error!("Row not found", table = shell.dio.cache_table_name, id = id).mark_not_found()
}
