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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenery::table::OpCondition;
    use ciborium::Value as CborValue;
    use vantage_vista::FilterOp;

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
