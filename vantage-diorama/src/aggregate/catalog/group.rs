//! A `group_by` declared by name: the per-group reduction comes from the
//! catalog.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{Column, VistaMetadata};

use super::{AggregationCatalog, AggregationSpec, ScalarAggregation};
use crate::aggregate::aggregation::{Reads, Rows};
use crate::aggregate::builtin::{GroupBy, GroupReducer};

/// The per-group reducer [`AggregationCatalog::group_by`] builds: one named
/// reduction over the group's rows, written under its alias.
pub struct GroupReduction {
    alias: String,
    reduction: ScalarAggregation,
}

impl GroupReducer for GroupReduction {
    fn columns(&self) -> Vec<Column> {
        vec![Column::new(self.alias.clone(), "f64")]
    }

    fn reduce(
        &self,
        _key: &CborValue,
        members: &[&Record<CborValue>],
        schema: &VistaMetadata,
    ) -> Record<CborValue> {
        let rows: Rows = members
            .iter()
            .enumerate()
            .map(|(i, record)| (i.to_string(), (*record).clone()))
            .collect();
        let mut out = Record::new();
        out.insert(self.alias.clone(), self.reduction.compute(&rows, schema));
        out
    }

    fn reads(&self) -> Reads {
        self.reduction.reads()
    }
}

impl AggregationCatalog {
    /// Build a `group_by`: one row per distinct value of `key_column`, each
    /// carrying the key plus the named reduction over that group's rows.
    ///
    /// The per-group reduction is built through this same catalog, so a
    /// grouped `sum` and a top-level `sum` cannot disagree — same reduction,
    /// different row set — and an application-registered aggregation is
    /// groupable the moment it is registered.
    pub fn group_by(
        &self,
        key_column: impl Into<String>,
        op: &str,
        alias: impl Into<String>,
        spec: AggregationSpec,
    ) -> Result<GroupBy<GroupReduction>> {
        // Built once, up front: a bad declaration must surface now, not once
        // per group on every recomputation.
        let reduction = self.build(op, spec)?;
        Ok(GroupBy::column(
            key_column,
            GroupReduction {
                alias: alias.into(),
                reduction,
            },
        ))
    }
}
