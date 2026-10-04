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
fn name_words_split_on_separators_and_case() {
    assert_eq!(
        name::name_words("lastName"),
        ["last", "name", "lastname"].map(String::from)
    );
    assert_eq!(
        name::name_words("billing.phone-number"),
        ["billing", "phone", "number", "billingphone", "phonenumber"].map(String::from)
    );
    assert_eq!(
        name::name_words("userID"),
        ["user", "id", "userid"].map(String::from)
    );
}

#[test]
fn name_guess_ignores_words_that_merely_contain_a_keyword() {
    let g = ValueGen::seeded(1);
    // Fell through to the type fallback: a lorem word, never a person name,
    // phone number or other multi-word value.
    for name in ["hostname", "filename", "hotel", "cityscape", "telescope"] {
        let v = g.value_for(&col(name, "string"));
        let word = text(&v);
        assert!(
            word.chars().all(|c| c.is_ascii_lowercase()),
            "{name} should fall through to a lorem word, got {word:?}"
        );
    }
}

#[test]
fn name_guess_matches_whole_words() {
    let g = ValueGen::seeded(2);
    for name in ["email", "contact_email", "billingEmail", "user-email"] {
        assert!(
            text(&g.value_for(&col(name, "string"))).contains('@'),
            "{name}"
        );
    }
    for name in ["phone", "mobile", "tel", "phone_number", "mobilePhone"] {
        let v = g.value_for(&col(name, "string"));
        assert!(
            text(&v).chars().any(|c| c.is_ascii_digit()),
            "{name}: {v:?}"
        );
    }
    for name in [
        "first_name",
        "firstName",
        "lastName",
        "surname",
        "username",
        "login",
        "name",
    ] {
        let v = g.value_for(&col(name, "string"));
        assert!(!text(&v).is_empty(), "{name}");
    }
    // `first_name` is a first name only: no space, unlike `Name()`'s "Dr. A B".
    let first = g.value_for(&col("first_name", "string"));
    assert!(!text(&first).contains(' '), "{first:?}");
}

#[test]
fn type_fallback_datetime_is_within_the_last_90_days_of_now() {
    // 2026-09-28T00:00:00Z
    let now = 1_790_553_600;
    let g = ValueGen::seeded(5).with_now(now);
    let lo = rfc3339(now - 90 * 86_400);
    let hi = rfc3339(now);
    for ty in ["date", "datetime", "timestamp"] {
        for _ in 0..50 {
            let v = g.value_for(&col("created", ty));
            let at = text(&v);
            assert!(
                at >= lo.as_str() && at <= hi.as_str(),
                "{at} outside {lo}..{hi}"
            );
        }
    }
}

#[test]
fn type_fallback_datetime_replays_under_a_pinned_now() {
    let draw = || {
        ValueGen::seeded(8)
            .with_now(1_790_553_600)
            .value_for(&col("created", "datetime"))
    };
    assert_eq!(draw(), draw());
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
        let from_pool = v.is_empty() || v == "John Smith" || v.contains('🌸') || v.len() >= 200;
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
