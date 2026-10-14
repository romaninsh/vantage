//! Record and metadata helpers, and one-clause queries.

use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::{Column, Condition, FilterOp, SortDirection, VistaMetadata, VistaQuery};

pub fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

pub fn record(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), text(v)))
        .collect()
}

pub fn meta(columns: &[&str]) -> VistaMetadata {
    let mut m = VistaMetadata::new();
    for c in columns {
        let col = if *c == "id" {
            Column::new("id", "String").with_flag("id")
        } else {
            Column::new(*c, "String")
        };
        m = m.with_column(col);
    }
    m.with_id_column("id")
}

// ---- one-clause queries, for `DeriveBuilder::with_query` ------------------

pub fn cond(column: &str, op: FilterOp, value: impl Into<CborValue>) -> VistaQuery {
    VistaQuery::new().with_condition(Condition::new(column, op, value))
}

pub fn search(text: &str) -> VistaQuery {
    VistaQuery::new().with_search(text)
}

pub fn order(column: &str, dir: SortDirection) -> VistaQuery {
    VistaQuery::new().with_order(column, dir)
}
