//! Reference ordering: a table referencing another must be generated after
//! it, so its reference columns can pick real parent ids. Ties keep
//! declaration order; a cycle among references is an error.

use std::collections::HashMap;

use vantage_core::{Result, VantageError, error};

use super::table::TableGen;

/// Indices into `tables`, in generation order: every table comes after every
/// table it [`reference`](super::table::TableGen::reference)s, and among
/// tables with no unmet dependency, declaration order is kept.
pub(super) fn generation_order(tables: &[TableGen]) -> Result<Vec<usize>> {
    let index: HashMap<&str, usize> = tables
        .iter()
        .enumerate()
        .map(|(i, t)| (t.name.as_str(), i))
        .collect();

    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); tables.len()];
    for (i, t) in tables.iter().enumerate() {
        for r in &t.refs {
            let Some(&j) = index.get(r.target.as_str()) else {
                return Err(error!(
                    "Column references an unknown table",
                    table = t.name,
                    column = r.column,
                    target = r.target
                )
                .mark_not_found());
            };
            deps[i].push(j);
        }
    }

    let mut placed = vec![false; tables.len()];
    let mut order = Vec::with_capacity(tables.len());
    while order.len() < tables.len() {
        let ready = (0..tables.len()).find(|&i| !placed[i] && deps[i].iter().all(|&j| placed[j]));
        match ready {
            Some(i) => {
                placed[i] = true;
                order.push(i);
            }
            None => return Err(cycle_error(tables, &deps, &placed)),
        }
    }
    Ok(order)
}

/// A cycle leaves every table on it permanently unready. Walk one dependency
/// edge at a time from an unplaced table until a table repeats, and report
/// that loop.
fn cycle_error(tables: &[TableGen], deps: &[Vec<usize>], placed: &[bool]) -> VantageError {
    let mut cur = (0..tables.len())
        .find(|&i| !placed[i])
        .expect("no progress means at least one table is unplaced");
    let mut seen_at = HashMap::new();
    let mut path: Vec<usize> = Vec::new();
    loop {
        if let Some(&pos) = seen_at.get(&cur) {
            let mut names: Vec<&str> = path[pos..]
                .iter()
                .map(|&i| tables[i].name.as_str())
                .collect();
            names.push(tables[cur].name.as_str());
            return error!("Tables form a reference cycle", cycle = names.join(" -> "));
        }
        seen_at.insert(cur, path.len());
        path.push(cur);
        cur = *deps[cur]
            .iter()
            .find(|&&j| !placed[j])
            .expect("an unplaced table depends on another unplaced table");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FakerColumn;

    #[test]
    fn keeps_declaration_order_when_independent() {
        let tables = vec![TableGen::new("a"), TableGen::new("b")];
        assert_eq!(generation_order(&tables).unwrap(), [0, 1]);
    }

    #[test]
    fn puts_referenced_table_first() {
        let tables = vec![
            TableGen::new("child")
                .column(FakerColumn::new("parent_id", "string"))
                .reference("parent_id", "parent"),
            TableGen::new("parent"),
        ];
        assert_eq!(generation_order(&tables).unwrap(), [1, 0]);
    }
}
