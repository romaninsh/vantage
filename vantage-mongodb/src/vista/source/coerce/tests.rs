use super::*;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn date(s: &str) -> Bson {
    Bson::DateTime(DateTime::parse_rfc3339_str(s).unwrap())
}

fn dec(s: &str) -> Bson {
    Bson::Decimal128(s.parse().unwrap())
}

#[test]
fn declared_parses_each_kind() {
    let oid = ObjectId::new();
    let cases = [
        (BsonKind::ObjectId, text(&oid.to_hex()), Bson::ObjectId(oid)),
        (
            BsonKind::DateTime,
            text("2026-10-10T12:00:00Z"),
            date("2026-10-10T12:00:00Z"),
        ),
        (
            BsonKind::DateTime,
            CborValue::Tag(0, Box::new(text("2026-10-10T12:00:00+02:00"))),
            date("2026-10-10T10:00:00Z"),
        ),
        (
            BsonKind::DateTime,
            CborValue::Tag(1, Box::new(CborValue::Integer(60i64.into()))),
            Bson::DateTime(DateTime::from_millis(60_000)),
        ),
        (
            BsonKind::DateTime,
            CborValue::Tag(1, Box::new(CborValue::Float(1.5))),
            Bson::DateTime(DateTime::from_millis(1_500)),
        ),
        (BsonKind::Decimal, text(" 12.50 "), dec("12.50")),
        (BsonKind::Decimal, CborValue::Integer(7i64.into()), dec("7")),
        (BsonKind::Decimal, CborValue::Float(0.25), dec("0.25")),
        (BsonKind::Decimal, CborValue::Null, Bson::Null),
        (BsonKind::Decimal, text(""), Bson::Null),
        (BsonKind::DateTime, text(""), Bson::Null),
        (BsonKind::ObjectId, text(""), Bson::Null),
        // Bignums (tags 2/3) and a decimal fraction (tag 4: 314 × 10^-2).
        (
            BsonKind::Decimal,
            CborValue::Tag(2, Box::new(CborValue::Bytes(vec![1, 0]))),
            dec("256"),
        ),
        (
            BsonKind::Decimal,
            CborValue::Tag(3, Box::new(CborValue::Bytes(vec![0xff]))),
            dec("-256"),
        ),
        (
            BsonKind::Decimal,
            CborValue::Tag(
                4,
                Box::new(CborValue::Array(vec![
                    CborValue::Integer((-2i64).into()),
                    CborValue::Integer(314i64.into()),
                ])),
            ),
            dec("3.14"),
        ),
    ];
    for (kind, value, expected) in cases {
        assert_eq!(declared("c", kind, &value).unwrap(), expected, "{value:?}");
    }
}

#[test]
fn declared_maps_arrays_element_wise() {
    let (a, b) = (ObjectId::new(), ObjectId::new());
    let value = CborValue::Array(vec![text(&a.to_hex()), CborValue::Null, text(&b.to_hex())]);
    assert_eq!(
        declared("tags", BsonKind::ObjectId, &value).unwrap(),
        Bson::Array(vec![Bson::ObjectId(a), Bson::Null, Bson::ObjectId(b)])
    );
}

#[test]
fn declared_refuses_what_does_not_parse() {
    let cases = [
        (BsonKind::ObjectId, text("not-hex")),
        (BsonKind::ObjectId, CborValue::Integer(1i64.into())),
        (BsonKind::DateTime, text("not a date")),
        (BsonKind::DateTime, CborValue::Integer(1i64.into())),
        (BsonKind::Decimal, text("twelve")),
        (BsonKind::Decimal, CborValue::Float(f64::NAN)),
        // Epoch seconds outside the millisecond range.
        (
            BsonKind::DateTime,
            CborValue::Tag(1, Box::new(CborValue::Float(1e300))),
        ),
        (
            BsonKind::DateTime,
            CborValue::Tag(1, Box::new(CborValue::Integer(i64::MAX.into()))),
        ),
        // Only the tags that spell the kind are read.
        (
            BsonKind::ObjectId,
            CborValue::Tag(0, Box::new(text("aaaaaaaaaaaaaaaaaaaaaaaa"))),
        ),
        (
            BsonKind::DateTime,
            CborValue::Tag(32, Box::new(text("2026-10-10T12:00:00Z"))),
        ),
        (
            BsonKind::Decimal,
            CborValue::Tag(1, Box::new(CborValue::Integer(5i64.into()))),
        ),
        (
            BsonKind::Decimal,
            CborValue::Array(vec![text("1"), text("x")]),
        ),
    ];
    for (kind, value) in cases {
        let err = declared("col", kind, &value).expect_err("refused");
        assert!(err.is_conflict(), "{err}");
        let msg = err.to_string();
        assert!(msg.contains("column type") && msg.contains("col"), "{msg}");
        assert!(msg.contains(kind.name()), "{msg}");
    }
}

#[test]
fn preserve_keeps_stored_type_when_text_parses() {
    let oid = ObjectId::new();
    let when = date("2026-10-10T12:00:00Z");
    let cases = [
        (
            Bson::ObjectId(oid),
            text(&oid.to_hex()),
            Bson::ObjectId(oid),
        ),
        (when.clone(), text("2026-10-10T12:00:00Z"), when.clone()),
        (dec("1.5"), text("2.75"), dec("2.75")),
        // Not parseable as the stored type: written as given.
        (Bson::ObjectId(oid), text("abc"), Bson::String("abc".into())),
        (when, text("soon"), Bson::String("soon".into())),
        // Stored as text: stays text even when it looks like an id.
        (
            Bson::String("x".into()),
            text(&oid.to_hex()),
            Bson::String(oid.to_hex()),
        ),
        (dec("1"), CborValue::Null, Bson::Null),
        // A stored Int32 stays one while the integer fits.
        (
            Bson::Int32(1),
            CborValue::Integer(42i64.into()),
            Bson::Int32(42),
        ),
        (
            Bson::Int32(1),
            CborValue::Integer(5_000_000_000i64.into()),
            Bson::Int64(5_000_000_000),
        ),
        (
            Bson::Int64(1),
            CborValue::Integer(42i64.into()),
            Bson::Int64(42),
        ),
    ];
    for (stored, value, expected) in cases {
        assert_eq!(
            preserve(&stored, &value),
            expected,
            "{stored:?} <- {value:?}"
        );
    }
}

#[test]
fn preserve_walks_documents_and_same_length_arrays() {
    let (a, b, c) = (ObjectId::new(), ObjectId::new(), ObjectId::new());
    let stored = Bson::Document(bson::doc! {
        "venue": a,
        "tags": [b, "plain"],
        "label": "hall",
    });
    let value = CborValue::Map(vec![
        (text("venue"), text(&c.to_hex())),
        (
            text("tags"),
            CborValue::Array(vec![text(&b.to_hex()), text("plain")]),
        ),
        (text("label"), text("room")),
        (text("extra"), text(&a.to_hex())),
    ]);
    assert_eq!(
        preserve(&stored, &value),
        Bson::Document(bson::doc! {
            "venue": c,
            "tags": [b, "plain"],
            "label": "room",
            "extra": a.to_hex(),
        })
    );

    // A resized array has no positional counterpart: written as given.
    let resized = CborValue::Array(vec![text(&b.to_hex())]);
    assert_eq!(
        preserve(
            &Bson::Array(vec![Bson::ObjectId(b), Bson::ObjectId(c)]),
            &resized
        ),
        Bson::Array(vec![Bson::String(b.to_hex())])
    );
}

#[test]
fn declared_over_keeps_unchanged_array_elements() {
    let (a, b) = (ObjectId::new(), ObjectId::new());
    // A legacy string sibling survives a changed element.
    let stored = Bson::Array(vec![Bson::String("c1".into()), Bson::ObjectId(a)]);
    let value = CborValue::Array(vec![text("c1"), text(&b.to_hex())]);
    assert_eq!(
        declared_over("tags", BsonKind::ObjectId, Some(&stored), &value).unwrap(),
        Bson::Array(vec![Bson::String("c1".into()), Bson::ObjectId(b)])
    );

    // A sibling DateTime that renders as `""` is kept, not nulled.
    let far = Bson::DateTime(DateTime::from_millis(i64::MAX));
    let stored = Bson::Array(vec![far.clone(), date("2026-10-10T12:00:00Z")]);
    let value = CborValue::Array(vec![text(""), text("2026-10-11T12:00:00Z")]);
    assert_eq!(
        declared_over("when", BsonKind::DateTime, Some(&stored), &value).unwrap(),
        Bson::Array(vec![far, date("2026-10-11T12:00:00Z")])
    );

    // A changed element is still checked; a resized array is checked whole.
    let stored = Bson::Array(vec![Bson::String("c1".into()), Bson::ObjectId(a)]);
    let changed = CborValue::Array(vec![text("c2"), text(&a.to_hex())]);
    assert!(declared_over("tags", BsonKind::ObjectId, Some(&stored), &changed).is_err());
    let resized = CborValue::Array(vec![text("c1")]);
    assert!(declared_over("tags", BsonKind::ObjectId, Some(&stored), &resized).is_err());
}
