use super::*;
use crate::ColumnGen;

fn columns() -> Vec<FakerColumn> {
    vec![
        FakerColumn::new("id", "string"),
        FakerColumn::new("name", "string"),
        FakerColumn::new("club", "string"),
        FakerColumn::new("region", "string"),
    ]
}

fn text(v: Option<&CborValue>) -> String {
    match v {
        Some(CborValue::Text(s)) => s.clone(),
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn seed_id_is_zero_padded() {
    assert_eq!(seed_id(42), "00000000000000000042");
}

#[test]
fn without_fan_out_matches_the_prime_stride_loop() {
    let refs = [Reference {
        column: "club".into(),
        parent_count: 7,
    }];
    let rows = relational_rows(&ValueGen::seeded(5), &columns(), "id", 30, &refs, None);

    // The loop vantage-ui ran before this API existed, on the same seed.
    let values = ValueGen::seeded(5);
    let expected: Vec<_> = (0..30)
        .map(|seq| {
            let id = seed_id(seq);
            let mut record = values.record_for(&columns(), "id", &id);
            let pool: Vec<String> = (0..7).map(seed_id).collect();
            let pick = pool[(seq * 7919 + 1) % pool.len()].clone();
            record.insert("club".into(), CborValue::Text(pick));
            (id, record)
        })
        .collect();
    assert_eq!(rows, expected);
}

#[test]
fn fan_out_gives_each_parent_a_contiguous_bounded_brood() {
    let refs = [
        Reference {
            column: "club".into(),
            parent_count: 12,
        },
        Reference {
            column: "region".into(),
            parent_count: 3,
        },
    ];
    let fan = FanOut {
        column: "club".into(),
        min: 2,
        max: 6,
    };
    let rows = relational_rows(
        &ValueGen::seeded(9),
        &columns(),
        "id",
        999,
        &refs,
        Some(&fan),
    );

    let parents: Vec<String> = rows.iter().map(|(_, r)| text(r.get("club"))).collect();
    let mut sizes = Vec::new();
    for (i, p) in parents.iter().enumerate() {
        if i == 0 || parents[i - 1] != *p {
            assert!(!parents[..i].contains(p), "children of {p} not contiguous");
            sizes.push(0);
        }
        *sizes.last_mut().unwrap() += 1;
    }
    assert_eq!(sizes.len(), 12, "every parent has children");
    assert!(sizes.iter().all(|n| (2..=6).contains(n)), "{sizes:?}");
    assert_eq!(rows.len(), sizes.iter().sum::<usize>());
    assert!(sizes.iter().any(|n| *n != sizes[0]), "counts should vary");

    // The other reference still strides.
    for (seq, (id, r)) in rows.iter().enumerate() {
        assert_eq!(*id, seed_id(seq));
        assert_eq!(text(r.get("region")), seed_id((seq * 7919 + 1) % 3));
    }
}

#[test]
fn fan_out_is_deterministic_for_a_seed() {
    let refs = [Reference {
        column: "club".into(),
        parent_count: 20,
    }];
    let fan = FanOut {
        column: "club".into(),
        min: 0,
        max: 10,
    };
    let build = || relational_rows(&ValueGen::seeded(4), &columns(), "id", 0, &refs, Some(&fan));
    assert_eq!(build(), build());
}

#[test]
fn tree_column_scales_to_the_relational_row_count() {
    let mut cols = columns();
    cols.push(
        FakerColumn::new("parent_id", "string").with_generator(ColumnGen::Tree {
            roots: 2,
            depth: 3,
            min_depth: None,
        }),
    );
    let rows = relational_rows(&ValueGen::seeded(1), &cols, "id", 50, &[], None);
    let roots = rows
        .iter()
        .filter(|(_, r)| r.get("parent_id") == Some(&CborValue::Null))
        .count();
    assert_eq!(roots, 2);
}

fn fan(min: usize, max: usize) -> FanOut {
    FanOut {
        column: "club".into(),
        min,
        max,
    }
}

fn club_ref(parent_count: usize) -> Vec<Reference> {
    vec![Reference {
        column: "club".into(),
        parent_count,
    }]
}

#[test]
fn fan_out_validate_rejects_inverted_range() {
    let err = fan(5, 2).validate().unwrap_err();
    assert!(err.contains("min 5 is above max 2"), "{err}");
    assert!(fan(2, 2).validate().is_ok());
    assert!(check_plan(&club_ref(3), Some(&fan(5, 2))).is_err());
}

#[test]
fn check_plan_rejects_fan_out_on_a_non_reference_column() {
    let refs = [Reference {
        column: "region".into(),
        parent_count: 3,
    }];
    let err = check_plan(&refs, Some(&fan(1, 2))).unwrap_err();
    assert!(err.contains("not a reference column"), "{err}");
}

#[test]
fn check_plan_rejects_fan_out_over_an_empty_pool() {
    let err = check_plan(&club_ref(0), Some(&fan(1, 2))).unwrap_err();
    assert!(err.contains("no rows"), "{err}");
}

#[test]
fn check_plan_accepts_good_plans() {
    assert!(check_plan(&club_ref(0), None).is_ok());
    assert!(check_plan(&club_ref(4), Some(&fan(0, 3))).is_ok());
}
