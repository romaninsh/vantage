use ciborium::Value as CborValue;
use vantage_vista::{Condition, FilterOp, Preview, SortDirection, VistaQuery};

use super::ReadPlan;
use crate::dio::shell::DioShell;

impl DioShell {
    /// A facade read hits the local cache or the narrowed master, so the
    /// query worth seeing is the **master's**, which is what filled that
    /// cache in the first place.
    ///
    /// Which clauses that master carries is not a given: `plan` offers them
    /// to it and keeps whatever it accepts, so the same condition is a
    /// server-side filter against one driver and an in-memory one against the
    /// next. The preview therefore reports the **planned** master, and notes
    /// what the facade evaluates itself (the clauses the master refused, and
    /// the Dio's own residual) and whether it evaluates them over the cache
    /// or over the narrowed master's rows. Reporting the
    /// bare master and every clause as local would put pushed-down filters in
    /// the wrong place.
    pub(super) fn planned_preview(&self) -> Preview {
        let ReadPlan { source, local } = match self.plan() {
            Ok(plan) => plan,
            Err(e) => {
                return Preview::new()
                    .with_note(format!("the master refused this handle's query: {e}"));
            }
        };
        let master = match &source {
            Some(narrowed) => narrowed.preview(),
            None => self.dio.master.preview(),
        };

        let mut applied = describe(&local);
        applied.extend(describe(&self.dio.residual));

        // The note holds only what the master's query leaves out: the
        // master's own note, then the clauses the facade applies over the
        // rows it reads, and where those rows come from.
        let over = match source {
            Some(_) => "the master's rows",
            None => "the cache",
        };
        let cached =
            (!applied.is_empty()).then(|| format!("applied over {over}: {}", applied.join(", ")));
        let note = match (master.note, cached) {
            (Some(own), Some(cached)) => Some(format!("{own}; {cached}")),
            (own, cached) => own.or(cached),
        };
        let mut preview = Preview::new();
        if let Some(query) = master.query {
            preview = preview.with_query(query);
        }
        if let Some(note) = note {
            preview = preview.with_note(note);
        }
        preview
    }
}

/// Each clause of `query` as a short human-readable phrase.
fn describe(query: &VistaQuery) -> Vec<String> {
    let mut phrases: Vec<String> = query.conditions.iter().map(condition).collect();
    if let Some(text) = &query.search {
        phrases.push(format!("search {text:?}"));
    }
    if let Some((column, dir)) = &query.order {
        let dir = match dir {
            SortDirection::Ascending => "asc",
            SortDirection::Descending => "desc",
        };
        phrases.push(format!("order by {column} {dir}"));
    }
    phrases
}

fn condition(c: &Condition) -> String {
    let op = match c.op {
        FilterOp::Eq => "=".to_string(),
        FilterOp::Ne => "!=".to_string(),
        FilterOp::Gt => ">".to_string(),
        FilterOp::Gte => ">=".to_string(),
        FilterOp::Lt => "<".to_string(),
        FilterOp::Lte => "<=".to_string(),
        FilterOp::InSet => "in".to_string(),
        FilterOp::NotInSet => "not in".to_string(),
        FilterOp::Like => "like".to_string(),
        // `FilterOp` is non-exhaustive: a later operator shows by its name.
        other => format!("{other:?}").to_lowercase(),
    };
    format!("{} {op} {}", c.column, value(&c.value))
}

/// `value` as a reader would write it: text quoted, numbers, bools and null
/// bare, arrays as `[a, b]`. Anything else falls back to its debug form.
fn value(value: &CborValue) -> String {
    match value {
        CborValue::Text(text) => format!("{text:?}"),
        CborValue::Integer(i) => i128::from(*i).to_string(),
        CborValue::Float(f) => f.to_string(),
        CborValue::Bool(b) => b.to_string(),
        CborValue::Null => "null".to_string(),
        CborValue::Array(items) => {
            let items: Vec<String> = items.iter().map(self::value).collect();
            format!("[{}]", items.join(", "))
        }
        other => format!("{other:?}"),
    }
}
