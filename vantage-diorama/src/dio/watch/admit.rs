//! Holding a pushed change against the Dio's query before it reaches the
//! cache.
//!
//! A narrowed master's change stream is never trusted to filter itself: many
//! backends push every change to the table, whatever the subscriber's
//! conditions. So a change is judged here, against everything the Dio is
//! narrowed by.

use vantage_vista::{VistaMetadata, VistaQuery};

use crate::dio::DioInner;
use crate::ops::ChangeEvent;

/// What a Dio's follow does with the changes its master pushes.
pub(super) enum Admit {
    /// Nothing narrows the Dio: every change applies as it is.
    All,
    /// A row that fails any of `queries` leaves the set, so it applies as a
    /// delete.
    Matching {
        queries: Vec<VistaQuery>,
        metadata: VistaMetadata,
    },
    /// The master holds native conditions, which can't be judged in memory:
    /// every change re-reads the set instead.
    Reread,
}

impl Admit {
    /// The rule for `inner`: a change is judged by everything the master
    /// accepted (its conditions and search), plus the residual conditions not
    /// on augmented columns, plus the residual search when the Dio has no
    /// augmentations. A pushed row doesn't carry its augmented values yet, so
    /// the residual clauses that could depend on them are left to the read
    /// path, which evaluates the residual over the hydrated rows.
    pub(super) fn for_dio(inner: &DioInner) -> Self {
        let accepted = inner.master.query();
        if !accepted.is_locally_evaluable() {
            return Admit::Reread;
        }
        let mut queries = Vec::new();
        if inner.master.is_narrowed() {
            queries.push(accepted.clone());
        }
        let mut residual = inner.without_augmented(&inner.residual);
        residual.order = None;
        if !residual.conditions.is_empty() || residual.search.is_some() {
            queries.push(residual);
        }
        if queries.is_empty() {
            Admit::All
        } else {
            Admit::Matching {
                queries,
                metadata: inner.metadata.clone(),
            }
        }
    }

    /// `change` as it applies to the Dio, or `None` when it can't be judged
    /// (always, under [`Admit::Reread`]) and the set must be re-read instead.
    pub(super) fn judge(&self, change: ChangeEvent) -> Option<ChangeEvent> {
        match self {
            Admit::All => Some(change),
            Admit::Reread => None,
            Admit::Matching { queries, metadata } => judge(queries, metadata, change),
        }
    }
}

/// `change` as it applies under `queries`, or `None` when its row can't be
/// judged.
fn judge(
    queries: &[VistaQuery],
    metadata: &VistaMetadata,
    change: ChangeEvent,
) -> Option<ChangeEvent> {
    let (id, row) = match &change {
        ChangeEvent::Inserted { id, new: Some(row) }
        | ChangeEvent::Updated { id, new: Some(row) } => (id, row),
        _ => return Some(change),
    };
    for query in queries {
        if !query.matches(row, metadata).ok()? {
            return Some(ChangeEvent::Deleted { id: id.clone() });
        }
    }
    Some(change)
}

#[cfg(test)]
mod tests {
    use ciborium::Value as CborValue;
    use vantage_types::Record;
    use vantage_vista::{Condition, FilterOp, VistaMetadata, VistaQuery};

    use super::{Admit, ChangeEvent};

    fn upsert(id: &str, team: &str) -> ChangeEvent {
        let row: Record<CborValue> = [("team".to_string(), CborValue::Text(team.into()))]
            .into_iter()
            .collect();
        ChangeEvent::Updated {
            id: id.into(),
            new: Some(row),
        }
    }

    /// Each change as judged, `reread` where it can't be.
    fn labels(admit: &Admit, changes: Vec<ChangeEvent>) -> Vec<String> {
        changes
            .into_iter()
            .map(|change| match admit.judge(change) {
                Some(ChangeEvent::Inserted { id, .. }) => format!("insert {id}"),
                Some(ChangeEvent::Updated { id, .. }) => format!("update {id}"),
                Some(ChangeEvent::Deleted { id }) => format!("delete {id}"),
                Some(ChangeEvent::Invalidated) => "invalidated".into(),
                None => "reread".into(),
            })
            .collect()
    }

    #[test]
    fn a_row_outside_the_query_applies_as_a_delete() {
        let admit = Admit::Matching {
            queries: vec![VistaQuery::new().with_condition(Condition::new(
                "team",
                FilterOp::Eq,
                "red",
            ))],
            metadata: VistaMetadata::new(),
        };
        let judged = labels(&admit, vec![upsert("a", "red"), upsert("b", "blue")]);
        assert_eq!(judged, ["update a", "delete b"]);
    }

    /// A row the evaluator can't judge asks for a re-read; a delete carries
    /// no row and applies as it is.
    #[test]
    fn an_unjudgeable_row_asks_for_a_reread() {
        let mut native = VistaQuery::new();
        native.native.push("opaque".into());
        let admit = Admit::Matching {
            queries: vec![native],
            metadata: VistaMetadata::new(),
        };
        let changes = vec![upsert("a", "red"), ChangeEvent::Deleted { id: "c".into() }];
        assert_eq!(labels(&admit, changes), ["reread", "delete c"]);
    }

    #[test]
    fn a_native_query_rereads_the_set() {
        let judged = labels(
            &Admit::Reread,
            vec![upsert("a", "red"), upsert("b", "blue")],
        );
        assert_eq!(judged, ["reread", "reread"]);
    }
}
