use ciborium::Value as CborValue;
use vantage_types::Record;

use super::SortDir;

/// Resolve a column key against a record, walking dotted paths (`obj.field`)
/// into nested CBOR `Map`s. A REST `?mode=detailed` response stores belongs-to
/// objects as nested maps (e.g. `launch_service_provider: { name: … }`) and
/// surfaces their leaves as dotted columns, so a flat `rec.get("a.b")` misses
/// them. Tries the literal key first (covers flat columns and any key that
/// genuinely contains a dot), then descends segment by segment.
pub(crate) fn record_get_path<'a>(rec: &'a Record<CborValue>, path: &str) -> Option<&'a CborValue> {
    if let Some(v) = rec.get(path) {
        return Some(v);
    }
    let mut segments = path.split('.');
    let mut current = rec.get(segments.next()?)?;
    for segment in segments {
        let CborValue::Map(entries) = current else {
            return None;
        };
        current = entries.iter().find_map(|(k, v)| match k {
            CborValue::Text(name) if name == segment => Some(v),
            _ => None,
        })?;
    }
    Some(current)
}

pub(crate) fn matches_conditions(rec: &Record<CborValue>, conds: &[(String, CborValue)]) -> bool {
    conds
        .iter()
        .all(|(col, expected)| match record_get_path(rec, col) {
            Some(v) => cbor_eq(v, expected),
            None => false,
        })
}

/// Local evaluation of the non-equality [`OpCondition`](super::OpCondition)
/// filters — the fallback for operators the master vista can't push down. A
/// missing column never matches (same convention as
/// [`matches_conditions`]).
pub(crate) fn matches_op_conditions(rec: &Record<CborValue>, conds: &[super::OpCondition]) -> bool {
    use vantage_vista::FilterOp;
    conds.iter().all(|cond| {
        let Some(cell) = record_get_path(rec, &cond.column) else {
            return false;
        };
        match cond.op {
            FilterOp::Eq => cbor_eq(cell, &cond.value),
            FilterOp::Ne => !cbor_eq(cell, &cond.value),
            FilterOp::Gt | FilterOp::Gte | FilterOp::Lt | FilterOp::Lte => cond
                .op
                .matches_ordering(cbor_cmp(Some(cell), Some(&cond.value))),
            FilterOp::InSet => set_members(&cond.value).any(|m| cbor_eq(cell, m)),
            FilterOp::NotInSet => !set_members(&cond.value).any(|m| cbor_eq(cell, m)),
        }
    })
}

/// Iterate the members of a set operand — the `CborValue::Array` payload of an
/// `InSet` / `NotInSet` condition. A non-array operand yields nothing (so an
/// `InSet` matches nothing and a `NotInSet` matches everything), matching the
/// "malformed filter fails closed for membership" intent.
fn set_members(value: &CborValue) -> std::slice::Iter<'_, CborValue> {
    const EMPTY: &[CborValue] = &[];
    match value {
        CborValue::Array(items) => items.iter(),
        _ => EMPTY.iter(),
    }
}

/// Order-insensitive cache-key fragment for a set of
/// [`OpCondition`](super::OpCondition)s, so a scenery narrowed by operators
/// gets a distinct ordered index. Empty set → empty string (no key cost for
/// the common no-op-filter case).
pub(crate) fn op_conditions_key(conds: &[super::OpCondition]) -> String {
    if conds.is_empty() {
        return String::new();
    }
    let mut frags: Vec<String> = conds.iter().map(|c| c.key_fragment()).collect();
    frags.sort();
    frags.join(";")
}

pub(crate) fn matches_search(rec: &Record<CborValue>, needle: Option<&str>) -> bool {
    let Some(needle) = needle else {
        return true;
    };
    let needle_lc = needle.to_lowercase();
    rec.values().any(|v| match v {
        CborValue::Text(s) => s.to_lowercase().contains(&needle_lc),
        _ => false,
    })
}

pub(crate) fn cbor_eq(a: &CborValue, b: &CborValue) -> bool {
    match (a, b) {
        (CborValue::Text(x), CborValue::Text(y)) => x == y,
        (CborValue::Integer(x), CborValue::Integer(y)) => x == y,
        (CborValue::Bool(x), CborValue::Bool(y)) => x == y,
        // Float and the rest fall back to format-string compare. Good
        // enough for v1's hand-rolled filter.
        _ => format!("{a:?}") == format!("{b:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenery::table::OpCondition;
    use vantage_vista::FilterOp;

    fn rec(status: &str, count: i64) -> Record<CborValue> {
        let mut r = Record::new();
        r.insert("status".to_string(), CborValue::Text(status.to_string()));
        r.insert("count".to_string(), CborValue::Integer(count.into()));
        r
    }

    fn text_array(items: &[&str]) -> CborValue {
        CborValue::Array(
            items
                .iter()
                .map(|s| CborValue::Text((*s).to_string()))
                .collect(),
        )
    }

    #[test]
    fn ne_filters_out_only_the_equal_value() {
        let conds = vec![OpCondition::new(
            "status",
            FilterOp::Ne,
            CborValue::Text("unregistered".into()),
        )];
        assert!(matches_op_conditions(&rec("registered", 1), &conds));
        assert!(!matches_op_conditions(&rec("unregistered", 1), &conds));
    }

    #[test]
    fn ordered_operators_compare_numerically() {
        let gt = vec![OpCondition::new(
            "count",
            FilterOp::Gt,
            CborValue::Integer(5.into()),
        )];
        assert!(matches_op_conditions(&rec("x", 6), &gt));
        assert!(!matches_op_conditions(&rec("x", 5), &gt));

        let lte = vec![OpCondition::new(
            "count",
            FilterOp::Lte,
            CborValue::Integer(5.into()),
        )];
        assert!(matches_op_conditions(&rec("x", 5), &lte));
        assert!(!matches_op_conditions(&rec("x", 6), &lte));
    }

    #[test]
    fn set_membership() {
        let in_set = vec![OpCondition::new(
            "status",
            FilterOp::InSet,
            text_array(&["registered", "pending"]),
        )];
        assert!(matches_op_conditions(&rec("registered", 1), &in_set));
        assert!(!matches_op_conditions(&rec("unregistered", 1), &in_set));

        let not_in = vec![OpCondition::new(
            "status",
            FilterOp::NotInSet,
            text_array(&["unregistered"]),
        )];
        assert!(matches_op_conditions(&rec("registered", 1), &not_in));
        assert!(!matches_op_conditions(&rec("unregistered", 1), &not_in));
    }

    #[test]
    fn missing_column_never_matches() {
        let conds = vec![OpCondition::new(
            "nonexistent",
            FilterOp::Ne,
            CborValue::Text("x".into()),
        )];
        assert!(!matches_op_conditions(&rec("registered", 1), &conds));
    }

    #[test]
    fn op_key_is_order_insensitive_and_operator_distinct() {
        let a = OpCondition::new("status", FilterOp::Ne, CborValue::Text("u".into()));
        let b = OpCondition::new("count", FilterOp::Gt, CborValue::Integer(5.into()));
        // Same set, different declared order → same key.
        assert_eq!(
            op_conditions_key(&[a.clone(), b.clone()]),
            op_conditions_key(&[b, a])
        );
        // Different operator → different key.
        let ne = OpCondition::new("status", FilterOp::Ne, CborValue::Text("u".into()));
        let eq = OpCondition::new("status", FilterOp::Eq, CborValue::Text("u".into()));
        assert_ne!(op_conditions_key(&[ne]), op_conditions_key(&[eq]));
    }
}

/// Row comparator for a sortable column that keeps **empty (`None`) values at
/// the end regardless of direction**. Only the present-vs-present comparison
/// follows `dir`; a missing value always sorts after a present one, so
/// flipping asc↔desc never lifts blanks to the top. Use this instead of
/// `cbor_cmp(...).reverse()`, which reverses null placement along with
/// everything else.
pub(crate) fn cmp_sort(
    a: Option<&CborValue>,
    b: Option<&CborValue>,
    dir: SortDir,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(_), Some(_)) => {
            let ord = cbor_cmp(a, b);
            match dir {
                SortDir::Asc => ord,
                SortDir::Desc => ord.reverse(),
            }
        }
    }
}

pub(crate) fn cbor_cmp(a: Option<&CborValue>, b: Option<&CborValue>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, _) => Ordering::Less,
        (_, None) => Ordering::Greater,
        (Some(lhs), Some(rhs)) => match (lhs, rhs) {
            (CborValue::Text(l), CborValue::Text(r)) => l.cmp(r),
            (CborValue::Integer(l), CborValue::Integer(r)) => i128::from(*l).cmp(&i128::from(*r)),
            // Floats (and int/float mixes) sort by numeric value — NOT by their
            // `{:?}` debug string, which would rank "657.96" above "1826.19".
            // NaN is treated as equal so a stray NaN can't panic the sort.
            (CborValue::Float(l), CborValue::Float(r)) => {
                l.partial_cmp(r).unwrap_or(Ordering::Equal)
            }
            (CborValue::Integer(l), CborValue::Float(r)) => (i128::from(*l) as f64)
                .partial_cmp(r)
                .unwrap_or(Ordering::Equal),
            (CborValue::Float(l), CborValue::Integer(r)) => l
                .partial_cmp(&(i128::from(*r) as f64))
                .unwrap_or(Ordering::Equal),
            (CborValue::Bool(l), CborValue::Bool(r)) => l.cmp(r),
            _ => format!("{lhs:?}").cmp(&format!("{rhs:?}")),
        },
    }
}

#[cfg(test)]
mod sort_tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn empty_values_sort_last_in_both_directions() {
        let one = CborValue::Integer(1i64.into());
        let two = CborValue::Integer(2i64.into());

        // Present values follow the direction.
        assert_eq!(
            cmp_sort(Some(&one), Some(&two), SortDir::Asc),
            Ordering::Less
        );
        assert_eq!(
            cmp_sort(Some(&one), Some(&two), SortDir::Desc),
            Ordering::Greater
        );

        // A missing value sorts AFTER a present one regardless of direction —
        // so flipping asc↔desc never lifts blanks to the top.
        for dir in [SortDir::Asc, SortDir::Desc] {
            assert_eq!(cmp_sort(None, Some(&one), dir), Ordering::Greater);
            assert_eq!(cmp_sort(Some(&one), None, dir), Ordering::Less);
            assert_eq!(cmp_sort(None, None, dir), Ordering::Equal);
        }
    }
}
