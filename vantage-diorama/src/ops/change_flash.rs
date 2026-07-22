//! `ChangeFlash` — the immutable outbound change, mount → vista.
//!
//! The mirror of [`ChangeEvent`](crate::ops::ChangeEvent): inbound light
//! paints the scenery, outbound light is a flash. One `ChangeFlash` is a
//! single unit of work for the write pipeline — servo → queue → worker →
//! routing (`on_flash`) or the default write-to-master path.
//!
//! A flash is frozen at fire time and self-contained: it carries the
//! `patch` (only the fields that changed), the `before` pre-image (what
//! the emitter saw), and derives the merged `after`. That is more than a
//! bare write op needs and exactly what optimistic rollback, routing
//! callbacks, and audit trails want.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::record::ActiveRecord;
use vantage_dataset::traits::WritableValueSet;
use vantage_types::Record;

/// What kind of change this flash carries.
///
/// `Replace` exists because a patch-merge cannot express field removal;
/// `Clear` (id-less, "delete every row") keeps its historical
/// no-optimism special-casing in the pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlashKind {
    Insert,
    Replace,
    Patch,
    Delete,
    Clear,
}

/// One frozen outbound change. See the module docs for the shape.
#[derive(Debug, Clone)]
pub struct ChangeFlash {
    kind: FlashKind,
    id: Option<String>,
    /// The fields that changed — the full record for `Insert`/`Replace`,
    /// the dirty set for `Patch`, empty for `Delete`/`Clear`.
    patch: Record<CborValue>,
    /// Pre-image at fire time. `None` when the row did not exist (an
    /// insert) or the emitter had no snapshot; the optimistic path fills
    /// it from the cache before routing.
    before: Option<Record<CborValue>>,
}

impl ChangeFlash {
    pub fn new(kind: FlashKind, id: Option<String>, patch: Record<CborValue>) -> Self {
        Self {
            kind,
            id,
            patch,
            before: None,
        }
    }

    /// A new row with a known id.
    pub fn insert(id: impl Into<String>, record: Record<CborValue>) -> Self {
        Self::new(FlashKind::Insert, Some(id.into()), record)
    }

    /// Full-record overwrite (drops fields absent from `record`).
    pub fn replace(id: impl Into<String>, record: Record<CborValue>) -> Self {
        Self::new(FlashKind::Replace, Some(id.into()), record)
    }

    /// Remove the row at `id`.
    pub fn delete(id: impl Into<String>) -> Self {
        Self::new(FlashKind::Delete, Some(id.into()), Record::new())
    }

    /// Remove every row. Id-less; runs straight through the write path
    /// with no optimistic staging.
    pub fn clear() -> Self {
        Self::new(FlashKind::Clear, None, Record::new())
    }

    /// Attach the pre-image the emitter observed at fire time.
    pub fn with_before(mut self, before: Record<CborValue>) -> Self {
        self.before = Some(before);
        self
    }

    /// Fill the pre-image if the emitter didn't supply one — the
    /// optimistic path calls this with the cache snapshot it took, so
    /// routing callbacks always see a complete flash.
    pub(crate) fn ensure_before(&mut self, before: Option<&Record<CborValue>>) {
        if self.before.is_none()
            && let Some(b) = before
        {
            self.before = Some(b.clone());
        }
    }

    pub fn kind(&self) -> &FlashKind {
        &self.kind
    }

    /// Id the flash targets. `Clear` returns `None`.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Only the fields that changed.
    pub fn patch(&self) -> &Record<CborValue> {
        &self.patch
    }

    /// Pre-image at fire time, when known.
    pub fn before(&self) -> Option<&Record<CborValue>> {
        self.before.as_ref()
    }

    /// The merged result of applying this flash: the record itself for
    /// `Insert`/`Replace`, `before + patch` for `Patch`, `None` for
    /// `Delete`/`Clear` (nothing remains).
    pub fn after(&self) -> Option<Record<CborValue>> {
        match self.kind {
            FlashKind::Insert | FlashKind::Replace => Some(self.patch.clone()),
            FlashKind::Patch => {
                let mut merged = self.before.clone().unwrap_or_default();
                for (k, v) in &self.patch {
                    merged.insert(k.clone(), v.clone());
                }
                Some(merged)
            }
            FlashKind::Delete | FlashKind::Clear => None,
        }
    }

    /// Bind this flash's merged record to a *different* destination
    /// dataset — the routing ergonomic for `on_flash` callbacks: take
    /// the change, re-bind it to wherever it should land, `save()`.
    ///
    /// Errors when the flash has no record shape to bind (`Delete`,
    /// `Clear`) or no id.
    pub fn active_record<'a, D>(&self, dest: &'a D) -> Result<ActiveRecord<'a, D>>
    where
        D: WritableValueSet<Value = CborValue> + ?Sized,
        D::Id: From<String>,
    {
        let id = self
            .id()
            .ok_or_else(|| vantage_core::error!("id-less flash cannot bind to a record"))?;
        let after = self.after().ok_or_else(|| {
            vantage_core::error!(
                "flash has no record shape to bind",
                kind = format!("{:?}", self.kind)
            )
        })?;
        Ok(ActiveRecord::new(D::Id::from(id.to_string()), after, dest))
    }
}
