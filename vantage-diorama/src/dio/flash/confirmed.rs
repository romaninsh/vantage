//! The last confirmed row of an id with flashes in flight.
//!
//! Flashes on one id stack: each stages over the one before it, so a later
//! flash's pre-image holds an earlier flash's value the master never
//! confirmed. Rolling back to that pre-image would leave the cache showing a
//! write that failed. Each id's [`Flight`] keeps the row the master last
//! confirmed instead: the cached row when the first flash staged, then each
//! commit's value. A rollback with no earlier flash still staged restores it.

use std::collections::BTreeSet;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;

use super::stage::{adopted, staged_value, write_row};
use crate::dio::DioInner;
use crate::dio::pending::PendingFlashGuard;
use crate::lens::CacheStatus;
use crate::ops::{ChangeFlash, FlashKind};

/// One id's flashes in flight. Lives in the pending registry and goes when
/// the id's last flash settles.
#[derive(Default)]
pub(crate) struct Flight {
    /// Flashes begun on the id and not yet settled.
    pub(crate) depth: usize,
    next: u64,
    /// The stage order of the staged flashes still in flight.
    staged: BTreeSet<u64>,
    /// The row the master last confirmed, set once the first flash stages.
    confirmed: Option<Option<Record<CborValue>>>,
    /// The confirmed row is only accepted, not confirmed by a read: a
    /// rollback restores it as `CacheStatus::Unconfirmed`.
    unconfirmed: bool,
}

impl Flight {
    /// A flash stages over `pre`; returns its place in the stage order.
    pub(crate) fn stage(&mut self, pre: Option<&Record<CborValue>>, unconfirmed: bool) -> u64 {
        let seq = self.next;
        self.next += 1;
        self.staged.insert(seq);
        if self.confirmed.is_none() {
            self.confirmed = Some(pre.cloned());
            self.unconfirmed = unconfirmed;
        }
        seq
    }

    /// A flash settled. `true` once none is left on the id.
    pub(crate) fn end(&mut self, seq: Option<u64>) -> bool {
        if let Some(seq) = seq {
            self.staged.remove(&seq);
        }
        self.depth = self.depth.saturating_sub(1);
        self.depth == 0
    }

    fn later_staged(&self, seq: u64) -> bool {
        self.staged.range(seq + 1..).next().is_some()
    }

    /// The row a failed flash at `seq` puts back, or `None` to leave the row
    /// to a later flash staged over it. With an earlier flash still staged
    /// that is `pre` (its staged value, which it settles itself); otherwise
    /// the last confirmed row, with whether it is only accepted.
    fn rollback_to(&self, seq: u64, pre: Option<&Record<CborValue>>) -> Option<Restore> {
        if self.later_staged(seq) {
            return None;
        }
        if self.staged.range(..seq).next().is_some() {
            return Some((pre.cloned(), false));
        }
        match &self.confirmed {
            Some(row) => Some((row.clone(), self.unconfirmed)),
            None => Some((pre.cloned(), false)),
        }
    }

    /// The flash at `seq` committed: its value over the last confirmed row,
    /// with the row the backend reported storing (`stored`) over that, is
    /// now the confirmed one. A patch of a row the cache didn't hold staged
    /// over the master's row, so with no confirmed row its own `staged`
    /// value is the confirmed one. With `whole`, `stored` is the whole row
    /// and is the confirmed one as it is; `accepted` marks it only accepted.
    /// `true` when it may re-assert its fields in the cache, which is only
    /// while no later flash is staged over it.
    fn confirm(
        &mut self,
        seq: u64,
        flash: &ChangeFlash,
        staged: Option<&Record<CborValue>>,
        stored: Option<&Record<CborValue>>,
        whole: bool,
        accepted: bool,
    ) -> bool {
        self.unconfirmed = accepted;
        let before = self.confirmed.take().flatten();
        let value = match before {
            _ if whole => stored.cloned(),
            None if *flash.kind() == FlashKind::Patch => staged.cloned(),
            before => staged_value(flash, before.as_ref()),
        };
        self.confirmed = Some(value.map(|row| adopted(row, stored)));
        !self.later_staged(seq)
    }
}

/// Roll a failed flash back: put back the row its [`Flight`] names, but only
/// while the cache still holds `staged`, the value this flash staged (a
/// writer that ignored the pending guard may have replaced it). `true` when
/// the row was restored.
pub(crate) async fn roll_back(
    inner: &DioInner,
    id: &str,
    pending: Option<&PendingFlashGuard>,
    pre: Option<&Record<CborValue>>,
    staged: Option<&Record<CborValue>>,
) -> Result<bool> {
    let target = match pending.and_then(|guard| Some((guard, guard.seq()?))) {
        Some((guard, seq)) => guard.flight(|flight| flight.rollback_to(seq, pre)),
        None => Some((pre.cloned(), false)),
    };
    let Some((target, unconfirmed)) = target else {
        return Ok(false);
    };
    if inner.cache.get_value(id).await?.as_ref() != staged {
        return Ok(false);
    }
    match target {
        Some(row) if unconfirmed => {
            inner
                .cache
                .insert_value_with_status(id, &row, CacheStatus::Unconfirmed)
                .await?
        }
        target => write_row(inner, id, target.as_ref()).await?,
    }
    Ok(true)
}

/// The row a rollback puts back (`None` removes it), and whether it goes
/// back as `CacheStatus::Unconfirmed`.
type Restore = (Option<Record<CborValue>>, bool);

/// Record a committed flash, which staged `staged` and which the backend
/// reported storing as `stored`, as confirmed. `true` when it may re-assert
/// its fields in the cache: no later flash on the id is staged over it.
/// `whole` says `stored` is the whole row, `accepted` that the backend only
/// accepted it (see [`Flight::confirm`]).
pub(crate) fn confirm(
    pending: Option<&PendingFlashGuard>,
    flash: &ChangeFlash,
    staged: Option<&Record<CborValue>>,
    stored: Option<&Record<CborValue>>,
    whole: bool,
    accepted: bool,
) -> bool {
    match pending.and_then(|guard| Some((guard, guard.seq()?))) {
        Some((guard, seq)) => {
            guard.flight(|flight| flight.confirm(seq, flash, staged, stored, whole, accepted))
        }
        None => true,
    }
}
