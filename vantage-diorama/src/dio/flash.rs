//! The one write: [`Dio::flash`] stages a [`ChangeFlash`] in the cache, sends
//! it through the Dio's write queue and waits for it to settle.
//!
//! This is what makes form edits feel instant: the new value is visible the
//! moment the user hits save (rows flip to
//! [`PendingWrite`](crate::RowStatus::PendingWrite)), and on failure the cache
//! pre-image is restored and the row flips to
//! [`WriteFailed`](crate::RowStatus::WriteFailed). Because every view reads
//! the one cache row, the edit reflects across every bound scenery at once.
//!
//! In this path the cache is framework-managed. A Lens writer should write
//! upstream; it may also touch the cache, but the stage and the settle are
//! what views observe.

pub(crate) mod confine;
mod stage;
mod unstage;

use ciborium::Value as CborValue;
use tokio::sync::oneshot;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet as _;
use vantage_types::Record;
use vantage_vista::VistaQuery;

use super::worker::QueuedFlash;
use super::{Dio, DioEvent, DioInner, DioShell};
use crate::ops::{ChangeFlash, FlashKind, FlashOutcome};
use confine::SetRule;
pub(crate) use stage::settle;
use unstage::Unstage;

impl Dio {
    /// Write `flash` and wait until it settles.
    ///
    /// 1. **Confinement.** The write must stay inside the Dio's set, the
    ///    master's narrowing plus the [`residual`](Self::residual): a record gets the set's
    ///    equality conditions filled in, and a record or patch result outside
    ///    the set, or a write to an id held by a row outside it, is refused
    ///    as `Conflict` before anything is staged. A patch of a row the set
    ///    doesn't hold is `NotFound`, and a delete of one writes nothing. A
    ///    `Clear` the write route can't confine by itself runs as one delete
    ///    per row in the set.
    /// 2. **Stage.** A flash with an id is staged in the cache and announced
    ///    as [`WritePending`](DioEvent::WritePending); its row is marked in
    ///    flight, so a [`reconcile`](Self::reconcile) leaves it alone. The
    ///    flash reaches the write route with its pre-image filled from the
    ///    cache when the caller supplied none.
    /// 3. **Write.** The flash joins the Dio's write queue. The queue runs one
    ///    write-through at a time, in order, through the Lens's writer or the
    ///    master, and settles it. A confirmed delete publishes
    ///    [`RecordRemoved`](DioEvent::RecordRemoved), a confirmed write of a
    ///    row the cache didn't hold [`RecordInserted`](DioEvent::RecordInserted),
    ///    and any other confirmed write [`RecordChanged`](DioEvent::RecordChanged).
    ///    A failed one is restored and publishes
    ///    [`WriteReverted`](DioEvent::WriteReverted).
    ///
    /// Returns once the write settled: the [`FlashOutcome`] carries the id the
    /// row settled under (the backend's, for
    /// [`insert_new`](ChangeFlash::insert_new) or a re-keyed insert), or the
    /// write's error after the rollback. A caller that stops waiting doesn't
    /// stop the write; it still lands and settles.
    pub async fn flash(&self, mut flash: ChangeFlash) -> Result<FlashOutcome> {
        let inner = &self.inner;
        if super::worker::writing_for(inner) {
            return Err(error!(
                "a Lens writer cannot flash the Dio it is writing for",
                table = inner.cache_table_name
            ));
        }
        let set = inner.write_set();
        let rule = SetRule::new(set, inner);
        if *flash.kind() == FlashKind::Clear && rule.narrows() {
            return self.clear_set().await;
        }

        let id = flash.id().map(str::to_string);
        let pending = id.clone().map(|id| inner.pending_flashes.begin(id));
        let pre = match &id {
            // The row is judged hydrated when the set names an augmented
            // value, so the rule sees the value and not the gap.
            Some(id) if rule.needs_hydration() => {
                inner.read_hydrated(inner.cache.as_ref(), id, set).await?
            }
            Some(id) => inner.cache.get_value(id).await?,
            None => None,
        };
        flash.ensure_before(pre.as_ref());
        let mut held = None;
        if rule.narrows() {
            if inner.lens.callbacks.writer.is_some() {
                rule.refuse_native()?;
            }
            match self.confine(&rule, &mut flash, pre.as_ref()).await? {
                Confined::Skip => return Ok(FlashOutcome::applied()),
                Confined::Write(row) => held = row,
            }
            flash.ensure_before(held.as_ref());
        }

        let permit = inner
            .write_queue
            .reserve()
            .await
            .map_err(|_| error!("Dio write queue closed", table = inner.cache_table_name))?;
        let (reply, settled) = oneshot::channel();
        let mut queued = QueuedFlash {
            flash,
            pre,
            staged: None,
            pending,
            reply,
            keep_alive: inner.clone(),
        };
        if let Some(id) = id {
            // Armed from before the stage until the flash is queued: a caller
            // cancelled, or a stage that fails, in between restores the row.
            let base = queued.pre.as_ref().or(held.as_ref());
            let staged = stage::staged_value(&queued.flash, base);
            let unstage = Unstage::new(
                inner,
                id.clone(),
                queued.pre.clone(),
                staged.clone(),
                queued.pending.take(),
            );
            stage::write_row(inner, &id, staged.as_ref()).await?;
            let kind = *queued.flash.kind();
            let _ = inner.event_bus.send(DioEvent::WritePending { id, kind });
            queued.staged = staged;
            queued.pending = unstage.disarm();
        }
        permit.send(queued);
        settled.await.unwrap_or_else(|_| {
            Err(error!(
                "the Dio write worker stopped before the write settled",
                table = inner.cache_table_name
            ))
        })
    }

    /// Hold `flash` to `rule`, filling its record from the set's invariants.
    ///
    /// The row a write is judged against is the cached one (hydrated when
    /// the rule names an augmented value), or the master's on a cache miss,
    /// which never carries augmented values: the Dio's augmentations run
    /// over it in memory first, so the whole rule judges it. It comes back
    /// with [`Confined::Write`] so a patch stages over the whole row. A patch
    /// of a row neither holds is `NotFound`; a delete of one is
    /// [`Confined::Skip`].
    async fn confine(
        &self,
        rule: &SetRule<'_>,
        flash: &mut ChangeFlash,
        pre: Option<&Record<CborValue>>,
    ) -> Result<Confined> {
        let mut found = None;
        if let Some(id) = flash.id().map(str::to_string) {
            let held = match pre {
                Some(row) => Some(row.clone()),
                None => match self.inner.master.get_value(id.as_str()).await? {
                    Some(row) if rule.needs_hydration() => {
                        Some(self.inner.augment_in_memory(row).await?)
                    }
                    held => held,
                },
            };
            match (flash.kind(), held) {
                (FlashKind::Patch, Some(before)) => {
                    rule.check_patch(&id, &before, flash.patch())?;
                    return Ok(Confined::Write(Some(before)));
                }
                (FlashKind::Patch, None) => {
                    return Err(error!(
                        "Row not found",
                        table = self.inner.cache_table_name,
                        id = id
                    )
                    .mark_not_found());
                }
                (FlashKind::Delete, held) => {
                    return Ok(match rule.not_outside(&id, held)? {
                        Some(row) => Confined::Write(Some(row)),
                        None => Confined::Skip,
                    });
                }
                (_, held) => found = rule.not_outside(&id, held)?,
            }
        }
        let conformed = rule.conform(flash.patch())?;
        flash.set_patch(conformed);
        Ok(Confined::Write(found))
    }

    /// Delete every row of the set, one confined delete each: the rows the
    /// Dio's facade reads, so on a paged Dio only the rows its cache holds.
    async fn clear_set(&self) -> Result<FlashOutcome> {
        let rows = DioShell::new(self.inner.clone()).read().await?;
        for id in rows.into_keys() {
            Box::pin(self.flash(ChangeFlash::delete(id))).await?;
        }
        Ok(FlashOutcome::applied())
    }
}

/// What confinement decided about a flash.
enum Confined {
    /// Nothing to write: a delete of a row the set doesn't hold.
    Skip,
    /// Write it. Carries the row the write was judged against, when one was
    /// read.
    Write(Option<Record<CborValue>>),
}

impl DioInner {
    /// The set a write must stay inside, beyond what its route enforces.
    ///
    /// Through the master, the narrowed master confines writes to the clauses
    /// it took, so only the residual is checked here. A Lens writer bypasses
    /// the master, so the Dio's whole query is checked, and a native
    /// condition in it refuses every write.
    ///
    /// The row a write is judged against is read through the narrowed
    /// master on a cache miss. Under a writer, a row the master holds outside
    /// the clauses it took reads as absent, so a full-record write to its id
    /// isn't recognised as taking it over; the writer decides where that
    /// write lands.
    pub(crate) fn write_set(&self) -> &VistaQuery {
        if self.lens.callbacks.writer.is_some() {
            &self.query
        } else {
            &self.residual
        }
    }
}
