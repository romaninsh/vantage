use super::*;

#[test]
fn merge_overwrites_master_columns_on_clash() {
    let rule = MergeRule::lift_all();
    let mut dest: Record<CborValue> = [("id".to_string(), CborValue::Text("master".into()))]
        .into_iter()
        .collect();
    let detail: Record<CborValue> = [
        ("id".to_string(), CborValue::Text("detail".into())),
        ("extra".to_string(), CborValue::Text("v".into())),
    ]
    .into_iter()
    .collect();

    rule.apply(&mut dest, &detail);

    // Detail wins on a clash (it's the authoritative hydration); new columns add.
    assert_eq!(dest.get("id"), Some(&CborValue::Text("detail".into())));
    assert_eq!(dest.get("extra"), Some(&CborValue::Text("v".into())));
}

#[test]
fn merge_respects_explicit_column_list() {
    let rule = MergeRule::lift(["extra"]);
    let mut dest: Record<CborValue> = Record::default();
    let detail: Record<CborValue> = [
        ("extra".to_string(), CborValue::Text("v".into())),
        ("skipme".to_string(), CborValue::Text("no".into())),
    ]
    .into_iter()
    .collect();

    rule.apply(&mut dest, &detail);

    assert_eq!(dest.get("extra"), Some(&CborValue::Text("v".into())));
    assert!(dest.get("skipme").is_none());
}
