use super::*;

fn col(name: &str, ty: &str) -> FakerColumn {
    FakerColumn::new(name, ty)
}

fn text(v: &CborValue) -> &str {
    match v {
        CborValue::Text(s) => s,
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn email_column_is_email_shaped() {
    let g = ValueGen::new();
    let v = g.value_for(&col("email", "string"));
    assert!(text(&v).contains('@'), "expected an email, got {v:?}");
}

#[test]
fn name_column_is_nonempty_text() {
    let g = ValueGen::new();
    let v = g.value_for(&col("full_name", "string"));
    assert!(!text(&v).is_empty());
}

#[test]
fn type_fallback_maps_scalars() {
    let g = ValueGen::new();
    assert!(matches!(
        g.value_for(&col("qty", "int")),
        CborValue::Integer(_)
    ));
    assert!(matches!(
        g.value_for(&col("balance", "decimal")),
        CborValue::Float(_)
    ));
    assert!(matches!(
        g.value_for(&col("active", "bool")),
        CborValue::Bool(_)
    ));
}

#[test]
fn record_sets_id_column_and_fills_rest() {
    let g = ValueGen::new();
    let cols = [
        col("id", "string"),
        col("email", "string"),
        col("age", "int"),
    ];
    let rec = g.record_for(&cols, "id", "abc");
    assert_eq!(rec.get("id"), Some(&CborValue::Text("abc".into())));
    assert!(text(rec.get("email").unwrap()).contains('@'));
    assert!(matches!(rec.get("age"), Some(CborValue::Integer(_))));
}

#[test]
fn same_seed_replays_the_same_records() {
    let cols = [
        col("id", "string"),
        col("name", "string"),
        col("age", "int"),
    ];
    let a: Vec<_> = {
        let g = ValueGen::seeded(42);
        (0..5)
            .map(|i| g.record_for(&cols, "id", &i.to_string()))
            .collect()
    };
    let b: Vec<_> = {
        let g = ValueGen::seeded(42);
        (0..5)
            .map(|i| g.record_for(&cols, "id", &i.to_string()))
            .collect()
    };
    assert_eq!(a, b, "a seed is a promise");
}

#[test]
fn weirdness_one_makes_every_string_cell_anomalous() {
    let g = ValueGen::seeded(7).with_weirdness(1.0);
    let cols = [col("id", "string"), col("name", "string")];
    let anomalies: Vec<String> = (0..40)
        .map(|i| {
            text(
                g.record_for(&cols, "id", &i.to_string())
                    .get("name")
                    .unwrap(),
            )
            .to_string()
        })
        .collect();
    // Every value came from the pool: blank, John Smith, unicode, or long.
    for v in &anomalies {
        let from_pool = v.is_empty() || v == "John Smith" || v.contains('Ž') || v.len() >= 200;
        assert!(from_pool, "unexpected non-anomalous value {v:?}");
    }
    // And the pool cycles — more than one kind appears over 40 draws.
    assert!(anomalies.iter().any(|v| v.is_empty()));
    assert!(anomalies.iter().any(|v| v.len() >= 200));
}

#[test]
fn weirdness_zero_never_draws_anomalies() {
    let g = ValueGen::seeded(7);
    let cols = [col("id", "string"), col("name", "string")];
    for i in 0..40 {
        let rec = g.record_for(&cols, "id", &i.to_string());
        let v = text(rec.get("name").unwrap()).to_string();
        assert!(
            !v.is_empty() && v != "John Smith" && v.len() < 200,
            "anomaly leaked: {v:?}"
        );
    }
}

#[test]
fn generator_wins_over_name_matching() {
    let g = ValueGen::seeded(3);
    let email = col("email", "string").with_generator(ColumnGen::Pattern("X-##".into()));
    let v = g.value_for(&email);
    assert!(text(&v).starts_with("X-"), "{v:?}");
}

#[test]
fn record_at_matches_record_for_without_generators() {
    let cols = [col("id", "string"), col("name", "string"), col("n", "int")];
    let a = ValueGen::seeded(11);
    let b = ValueGen::seeded(11);
    for i in 0..10 {
        let id = i.to_string();
        assert_eq!(
            a.record_for(&cols, "id", &id),
            b.record_at(&cols, "id", &id, i)
        );
    }
}
