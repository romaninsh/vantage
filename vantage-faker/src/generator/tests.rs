//! Generators driven through [`ValueGen`], plus the config shape.

use ciborium::Value as CborValue;

use super::{ColumnGen, Spread};
use crate::{FakerColumn, ValueGen};

/// 2026-09-28T00:00:00Z.
const NOW: i64 = 1_790_553_600;

fn column(generator: ColumnGen) -> Vec<FakerColumn> {
    vec![
        FakerColumn::new("id", "string"),
        FakerColumn::new("v", "string").with_generator(generator),
    ]
}

fn series(values: &ValueGen, generator: ColumnGen, rows: usize) -> Vec<CborValue> {
    let cols = column(generator);
    (0..rows)
        .map(|seq| {
            let rec = values.record_at(&cols, "id", &crate::seed_id(seq), seq);
            rec.get("v").cloned().unwrap()
        })
        .collect()
}

fn text(v: &CborValue) -> &str {
    match v {
        CborValue::Text(s) => s,
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn even_dates_are_monotonic_and_inside_bounds() {
    let generator = ColumnGen::Date {
        from: "-30d".into(),
        to: "now".into(),
        spread: Spread::Even,
    };
    let values = ValueGen::seeded(1).with_now(NOW).with_rows(200);
    let dates = series(&values, generator, 200);
    let dates: Vec<&str> = dates.iter().map(text).collect();
    assert!(dates.windows(2).all(|w| w[0] <= w[1]), "not ordered by seq");
    assert!(dates[0] >= "2026-08-29T00:00:00Z", "{}", dates[0]);
    assert!(dates[199] <= "2026-09-28T00:00:00Z");
    assert!(dates[199] >= "2026-09-27T00:00:00Z", "{}", dates[199]);
}

#[test]
fn random_dates_stay_inside_iso_bounds() {
    let generator = ColumnGen::Date {
        from: "2025-01-01".into(),
        to: "2025-03-01T12:00".into(),
        spread: Spread::Random,
    };
    for d in series(&ValueGen::seeded(2), generator, 500) {
        let d = text(&d);
        assert!(
            ("2025-01-01T00:00:00Z"..="2025-03-01T12:00:00Z").contains(&d),
            "{d}"
        );
        assert_eq!(d.len(), 20, "{d}");
    }
}

#[test]
fn walk_is_deterministic_across_builds() {
    let walk = ColumnGen::Walk {
        start: 100.0,
        step: 5.0,
        min: Some(80.0),
        max: Some(120.0),
        decimals: Some(1),
    };
    let a = series(&ValueGen::seeded(8), walk.clone(), 300);
    let b = series(&ValueGen::seeded(8), walk.clone(), 300);
    assert_eq!(a, b);
    assert!(
        a.iter()
            .all(|v| matches!(v, CborValue::Float(f) if (80.0..=120.0).contains(f)))
    );
    assert_ne!(a, series(&ValueGen::seeded(9), walk, 300));
}

#[test]
fn tree_roots_are_null_and_parents_precede_children() {
    let values = ValueGen::seeded(3).with_rows(40);
    let parents = series(&values, ColumnGen::Tree { roots: 4, depth: 3 }, 40);
    for (seq, p) in parents.iter().enumerate() {
        match p {
            CborValue::Null => assert!(seq < 4),
            CborValue::Text(id) => assert!(id.parse::<usize>().unwrap() < seq),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn deserializes_from_yaml() {
    let yaml = r#"
- pick: { values: [open, closed], weights: [3, 1] }
- range: { min: 1, max: 5 }
- date: { from: -90d, spread: even }
- sentence: { min_words: 3, max_words: 8 }
- pattern: "BA####"
- walk: { start: 100, step: 2.5, min: 0, decimals: 2 }
- tree: { roots: 5, depth: 4 }
"#;
    let parsed: Vec<ColumnGen> = serde_yaml_ng::from_str(yaml).unwrap();
    assert_eq!(
        parsed[2],
        ColumnGen::Date {
            from: "-90d".into(),
            to: "now".into(),
            spread: Spread::Even,
        }
    );
    assert_eq!(parsed[4], ColumnGen::Pattern("BA####".into()));
    assert!(parsed.iter().all(|g| g.validate().is_ok()));
    for bad in [
        "range: { min: 1, max: 2, typo: 3 }",
        "{ pattern: 'A#', range: { min: 1, max: 2 } }",
        "{}",
        "nonsense: 1",
    ] {
        assert!(serde_yaml_ng::from_str::<ColumnGen>(bad).is_err(), "{bad}");
    }
    let json: ColumnGen = serde_json::from_str(r#"{"tree":{"roots":3,"depth":2}}"#).unwrap();
    assert_eq!(json, ColumnGen::Tree { roots: 3, depth: 2 });
}

#[test]
fn validate_rejects_bad_parameters() {
    let bad = [
        ColumnGen::Pick {
            values: vec![],
            weights: None,
        },
        ColumnGen::Range {
            min: 5.0,
            max: 1.0,
            decimals: None,
        },
        ColumnGen::Date {
            from: "last tuesday".into(),
            to: "now".into(),
            spread: Spread::Random,
        },
    ];
    for g in bad {
        assert!(g.validate().is_err(), "{g:?}");
    }
}

fn date(from: &str, to: &str) -> ColumnGen {
    ColumnGen::Date {
        from: from.into(),
        to: to.into(),
        spread: Spread::Even,
    }
}

#[test]
fn validate_rejects_inverted_dates() {
    let err = date("now", "-30d").validate().unwrap_err();
    assert!(err.contains("later than"), "{err}");
    assert!(date("2026-02-01", "2026-01-01").validate().is_err());
    assert!(date("+1d", "2000-01-01").validate().is_err());
    assert!(date("-30d", "now").validate().is_ok());
    assert!(date("2000-01-01", "+1d").validate().is_ok());
}

#[test]
fn validate_rejects_empty_trees() {
    let err = ColumnGen::Tree { roots: 0, depth: 3 }
        .validate()
        .unwrap_err();
    assert!(err.contains("roots"), "{err}");
    let err = ColumnGen::Tree { roots: 3, depth: 0 }
        .validate()
        .unwrap_err();
    assert!(err.contains("depth"), "{err}");
    assert!(ColumnGen::Tree { roots: 1, depth: 1 }.validate().is_ok());
}
