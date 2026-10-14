#![cfg(feature = "serde")]

use ciborium::Value as CborValue;
use serde_json::{Value as JsonValue, json};
use vantage_types::{
    CborDialect, PlainDialect, PresentationDialect, Record, cbor_id_to_string, cbor_to_json,
    cbor_to_string, hex_encode, json_to_cbor, json_to_cbor_with_hint, record_to_json,
    rfc3339_to_tag12, tag12_to_rfc3339,
};

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn int(i: i64) -> CborValue {
    CborValue::Integer(i.into())
}

fn tagged(tag: u64, inner: CborValue) -> CborValue {
    CborValue::Tag(tag, Box::new(inner))
}

fn record_id(table: &str, id: CborValue) -> CborValue {
    tagged(8, CborValue::Array(vec![text(table), id]))
}

fn epoch_pair(secs: i64, nanos: i64) -> CborValue {
    tagged(12, CborValue::Array(vec![int(secs), int(nanos)]))
}

// cbor_to_json with PlainDialect

#[test]
fn scalars_convert_directly() {
    assert_eq!(
        cbor_to_json(&PlainDialect, CborValue::Null),
        JsonValue::Null
    );
    assert_eq!(
        cbor_to_json(&PlainDialect, CborValue::Bool(true)),
        json!(true)
    );
    assert_eq!(cbor_to_json(&PlainDialect, int(42)), json!(42));
    assert_eq!(cbor_to_json(&PlainDialect, int(-7)), json!(-7));
    assert_eq!(
        cbor_to_json(&PlainDialect, CborValue::Float(1.5)),
        json!(1.5)
    );
    assert_eq!(cbor_to_json(&PlainDialect, text("hi")), json!("hi"));
}

#[test]
fn u64_beyond_i64_stays_a_number() {
    assert_eq!(
        cbor_to_json(&PlainDialect, CborValue::Integer(u64::MAX.into())),
        json!(u64::MAX)
    );
}

#[test]
fn integers_below_i64_min_become_decimal_strings() {
    let neg = i128::from(i64::MIN) - 1;
    let value = CborValue::Integer(neg.try_into().unwrap());
    assert_eq!(
        cbor_to_json(&PlainDialect, value),
        JsonValue::String(neg.to_string())
    );
}

#[test]
fn non_finite_floats_become_null() {
    assert_eq!(
        cbor_to_json(&PlainDialect, CborValue::Float(f64::NAN)),
        JsonValue::Null
    );
    assert_eq!(
        cbor_to_json(&PlainDialect, CborValue::Float(f64::INFINITY)),
        JsonValue::Null
    );
    assert_eq!(
        cbor_to_json(&PlainDialect, CborValue::Float(f64::NEG_INFINITY)),
        JsonValue::Null
    );
}

#[test]
fn bytes_become_lowercase_hex() {
    assert_eq!(
        cbor_to_json(&PlainDialect, CborValue::Bytes(vec![0xde, 0xad, 0x01])),
        json!("dead01")
    );
}

#[test]
fn tags_are_dropped_by_default() {
    assert_eq!(
        cbor_to_json(&PlainDialect, tagged(999, text("hi"))),
        json!("hi")
    );
    assert_eq!(
        cbor_to_json(&PlainDialect, tagged(1, tagged(2, int(3)))),
        json!(3)
    );
}

#[test]
fn arrays_convert_element_wise() {
    let v = CborValue::Array(vec![int(1), text("two"), CborValue::Null]);
    assert_eq!(cbor_to_json(&PlainDialect, v), json!([1, "two", null]));
}

#[test]
fn tagged_value_survives_inside_a_map() {
    let v = CborValue::Map(vec![(text("id"), tagged(8, text("user:1")))]);
    assert_eq!(cbor_to_json(&PlainDialect, v), json!({"id": "user:1"}));
}

#[test]
fn map_keys_are_stringified() {
    let v = CborValue::Map(vec![
        (text("a"), CborValue::Bool(true)),
        (int(7), CborValue::Bool(false)),
    ]);
    assert_eq!(
        cbor_to_json(&PlainDialect, v),
        json!({"a": true, "7": false})
    );
}

#[test]
fn other_map_keys_render_via_debug() {
    let key = CborValue::Bool(true);
    let expected = format!("{key:?}");
    let v = CborValue::Map(vec![(key, int(1))]);
    let rendered = cbor_to_json(&PlainDialect, v);
    assert_eq!(
        rendered.as_object().unwrap().get(&expected),
        Some(&json!(1))
    );
}

#[test]
fn json_round_trips_through_cbor() {
    let value = json!({
        "s": "text",
        "i": 42,
        "neg": -5,
        "u": u64::MAX,
        "f": 1.25,
        "b": true,
        "n": null,
        "arr": [1, "two", {"nested": false}],
    });
    assert_eq!(
        cbor_to_json(&PlainDialect, json_to_cbor(value.clone())),
        value
    );
}

// custom dialects

#[test]
fn dialect_hooks_override_rendering() {
    struct Custom;
    impl CborDialect for Custom {
        fn bytes_to_json(&self, bytes: Vec<u8>) -> JsonValue {
            JsonValue::String(format!("b64:{}", bytes.len()))
        }
        fn tag_to_json(&self, tag: u64, inner: CborValue) -> JsonValue {
            match (tag, inner) {
                (8, CborValue::Text(s)) => JsonValue::String(format!("thing:{s}")),
                (_, inner) => cbor_to_json(self, inner),
            }
        }
        fn float_to_json(&self, f: f64) -> JsonValue {
            JsonValue::String(format!("float:{f}"))
        }
        fn big_int_to_json(&self, n: i128) -> JsonValue {
            JsonValue::String(format!("big:{n}"))
        }
        fn map_key_to_string(&self, key: CborValue) -> String {
            format!("key:{}", cbor_to_string(&PlainDialect, &key))
        }
    }

    assert_eq!(
        cbor_to_json(&Custom, CborValue::Bytes(vec![1, 2, 3])),
        json!("b64:3")
    );
    assert_eq!(
        cbor_to_json(&Custom, tagged(8, text("x"))),
        json!("thing:x")
    );
    // Unhandled tags still recurse with the same dialect.
    assert_eq!(
        cbor_to_json(&Custom, tagged(1, CborValue::Bytes(vec![1, 2]))),
        json!("b64:2")
    );
    assert_eq!(
        cbor_to_json(&Custom, CborValue::Float(f64::NAN)),
        json!("float:NaN")
    );
    let neg = i128::from(i64::MIN) - 1;
    assert_eq!(
        cbor_to_json(&Custom, CborValue::Integer(neg.try_into().unwrap())),
        JsonValue::String(format!("big:{neg}"))
    );
    assert_eq!(
        cbor_to_json(&Custom, CborValue::Map(vec![(text("a"), int(1))])),
        json!({"key:a": 1})
    );
}

#[test]
fn dialects_work_as_trait_objects() {
    let dialect: &dyn CborDialect = &PlainDialect;
    assert_eq!(
        cbor_to_json(dialect, CborValue::Bytes(vec![0xff])),
        json!("ff")
    );
    let dialect: &dyn CborDialect = &PresentationDialect;
    assert_eq!(
        cbor_to_json(dialect, tagged(6, CborValue::Null)),
        JsonValue::Null
    );
}

// PresentationDialect

#[test]
fn presentation_renders_record_ids() {
    assert_eq!(
        cbor_to_json(&PresentationDialect, record_id("client", text("abc"))),
        json!("client:abc")
    );
    assert_eq!(
        cbor_to_json(&PresentationDialect, record_id("user", int(42))),
        json!("user:42")
    );
}

#[test]
fn presentation_record_id_with_non_text_table_stays_an_array() {
    let v = tagged(8, CborValue::Array(vec![int(1), text("a")]));
    assert_eq!(cbor_to_json(&PresentationDialect, v), json!([1, "a"]));
}

#[test]
fn presentation_renders_none_as_null() {
    assert_eq!(
        cbor_to_json(&PresentationDialect, tagged(6, CborValue::Null)),
        JsonValue::Null
    );
}

#[test]
fn presentation_keeps_inner_text_of_displayable_tags() {
    for tag in [0, 9, 10, 13] {
        assert_eq!(
            cbor_to_json(&PresentationDialect, tagged(tag, text("12.34"))),
            json!("12.34"),
            "tag {tag}"
        );
    }
}

#[test]
fn presentation_renders_binary_uuid_as_hex() {
    let uuid = tagged(37, CborValue::Bytes(vec![0xab, 0xcd]));
    assert_eq!(cbor_to_json(&PresentationDialect, uuid), json!("abcd"));
}

#[test]
fn presentation_falls_back_to_plain_for_off_shape_tags() {
    assert_eq!(
        cbor_to_json(&PresentationDialect, tagged(0, int(5))),
        json!(5)
    );
    assert_eq!(
        cbor_to_json(&PresentationDialect, tagged(37, text("x"))),
        json!("x")
    );
    assert_eq!(
        cbor_to_json(&PresentationDialect, tagged(8, text("user:1"))),
        json!("user:1")
    );
    assert_eq!(
        cbor_to_json(&PresentationDialect, tagged(55, text("other"))),
        json!("other")
    );
}

#[test]
fn presentation_renders_epoch_pair_as_lossless_rfc3339() {
    let rendered = cbor_to_json(&PresentationDialect, epoch_pair(1_000_000_000, 5));
    let JsonValue::String(s) = rendered else {
        panic!("expected a string");
    };
    assert!(s.starts_with("2001-09-09T01:46:40"), "{s}");
    assert!(s.contains(".000000005"), "{s}");
    assert!(s.ends_with('Z'), "{s}");
}

#[test]
fn presentation_renders_malformed_epoch_pair_plainly() {
    let v = tagged(12, text("not a pair"));
    assert_eq!(cbor_to_json(&PresentationDialect, v), json!("not a pair"));
}

// record_to_json / cbor_to_string

#[test]
fn record_to_json_converts_every_field() {
    let mut record = Record::new();
    record.insert("name".to_string(), text("Alice"));
    record.insert("id".to_string(), tagged(8, text("user:1")));
    assert_eq!(
        record_to_json(&PlainDialect, record),
        json!({"name": "Alice", "id": "user:1"})
    );
}

#[test]
fn empty_record_is_an_empty_object() {
    assert_eq!(record_to_json(&PlainDialect, Record::new()), json!({}));
}

#[test]
fn cbor_to_string_renders_scalars_bare() {
    assert_eq!(cbor_to_string(&PlainDialect, &text("abc")), "abc");
    assert_eq!(cbor_to_string(&PlainDialect, &int(42)), "42");
    assert_eq!(cbor_to_string(&PlainDialect, &CborValue::Float(1.5)), "1.5");
    assert_eq!(
        cbor_to_string(&PlainDialect, &CborValue::Bool(true)),
        "true"
    );
    assert_eq!(cbor_to_string(&PlainDialect, &CborValue::Null), "");
}

#[test]
fn cbor_to_string_renders_compound_values_as_json() {
    assert_eq!(
        cbor_to_string(&PlainDialect, &CborValue::Array(vec![int(1)])),
        "[1]"
    );
    assert_eq!(
        cbor_to_string(
            &PresentationDialect,
            &CborValue::Array(vec![record_id("user", int(1))])
        ),
        "[\"user:1\"]"
    );
}

// json_to_cbor

#[test]
fn json_scalars_convert_to_cbor() {
    assert_eq!(json_to_cbor(json!(null)), CborValue::Null);
    assert_eq!(json_to_cbor(json!(true)), CborValue::Bool(true));
    assert_eq!(json_to_cbor(json!(-3)), int(-3));
    assert_eq!(
        json_to_cbor(json!(u64::MAX)),
        CborValue::Integer(u64::MAX.into())
    );
    assert_eq!(json_to_cbor(json!(1.5)), CborValue::Float(1.5));
    assert_eq!(json_to_cbor(json!("s")), text("s"));
}

#[test]
fn json_containers_convert_to_cbor() {
    assert_eq!(
        json_to_cbor(json!([1, "a"])),
        CborValue::Array(vec![int(1), text("a")])
    );
    assert_eq!(
        json_to_cbor(json!({"k": 1})),
        CborValue::Map(vec![(text("k"), int(1))])
    );
}

// json_to_cbor_with_hint

#[test]
fn no_hint_matches_lossless() {
    for v in [
        json!("s"),
        json!(7),
        json!(1.5),
        json!(true),
        json!(null),
        json!([1, 2]),
        json!({"a": 1}),
    ] {
        assert_eq!(json_to_cbor_with_hint(&v, None), json_to_cbor(v.clone()));
    }
}

#[test]
fn record_id_round_trips_under_hint() {
    let thing = record_id("golf_course", text("pebble_beach"));
    let rendered = cbor_to_json(&PresentationDialect, thing.clone());
    assert_eq!(rendered, json!("golf_course:pebble_beach"));
    assert_eq!(json_to_cbor_with_hint(&rendered, Some(&thing)), thing);
}

#[test]
fn record_id_with_colon_in_id_splits_on_the_hint_table() {
    let thing = record_id("event", text("2026-07-23:slot"));
    assert_eq!(
        json_to_cbor_with_hint(&json!("event:2026-07-23:slot"), Some(&thing)),
        thing
    );
}

#[test]
fn record_id_splits_on_first_colon_when_table_differs() {
    let hint = record_id("user", text("1"));
    assert_eq!(
        json_to_cbor_with_hint(&json!("other:5"), Some(&hint)),
        record_id("other", text("5"))
    );
}

#[test]
fn record_id_restores_numeric_id_part() {
    let thing = record_id("user", int(42));
    assert_eq!(
        json_to_cbor_with_hint(&json!("user:42"), Some(&thing)),
        thing
    );
    assert_eq!(
        json_to_cbor_with_hint(&json!("user:abc"), Some(&thing)),
        record_id("user", text("abc"))
    );
}

#[test]
fn record_id_with_single_text_hint_keeps_single_text_shape() {
    let hint = tagged(8, text("user:1"));
    assert_eq!(
        json_to_cbor_with_hint(&json!("user:2"), Some(&hint)),
        tagged(8, text("user:2"))
    );
}

#[test]
fn epoch_pair_survives_an_untouched_round_trip() {
    let hint = epoch_pair(1_816_421_400, 123_456_789);
    let rendered = cbor_to_json(&PresentationDialect, hint.clone());
    assert_eq!(json_to_cbor_with_hint(&rendered, Some(&hint)), hint);
}

#[test]
fn one_element_epoch_pair_keeps_its_shape_when_untouched() {
    let hint = tagged(12, CborValue::Array(vec![int(1_816_421_400)]));
    let rendered = cbor_to_json(&PresentationDialect, hint.clone());
    assert_eq!(json_to_cbor_with_hint(&rendered, Some(&hint)), hint);
}

#[test]
fn edited_epoch_pair_re_encodes_canonically() {
    let hint = epoch_pair(0, 0);
    assert_eq!(
        json_to_cbor_with_hint(&json!("2001-09-09T01:46:40.000000005Z"), Some(&hint)),
        epoch_pair(1_000_000_000, 5)
    );
}

#[test]
fn unparseable_edit_under_epoch_pair_hint_stays_text() {
    let hint = epoch_pair(0, 0);
    assert_eq!(
        json_to_cbor_with_hint(&json!("not a date"), Some(&hint)),
        text("not a date")
    );
}

#[test]
fn text_carrying_tag_re_wraps_under_hint() {
    for tag in [0, 9, 10, 13] {
        let hint = tagged(tag, text("old"));
        assert_eq!(
            json_to_cbor_with_hint(&json!("new"), Some(&hint)),
            tagged(tag, text("new")),
            "tag {tag}"
        );
    }
}

#[test]
fn tag_with_non_text_payload_does_not_re_wrap() {
    let hint = tagged(77, int(1));
    assert_eq!(
        json_to_cbor_with_hint(&json!("new"), Some(&hint)),
        text("new")
    );
}

#[test]
fn strings_coerce_to_the_hinted_scalar() {
    assert_eq!(json_to_cbor_with_hint(&json!("12"), Some(&int(0))), int(12));
    assert_eq!(
        json_to_cbor_with_hint(&json!("1.5"), Some(&CborValue::Float(0.0))),
        CborValue::Float(1.5)
    );
    assert_eq!(
        json_to_cbor_with_hint(&json!("true"), Some(&CborValue::Bool(false))),
        CborValue::Bool(true)
    );
    assert_eq!(
        json_to_cbor_with_hint(&json!("false"), Some(&CborValue::Bool(true))),
        CborValue::Bool(false)
    );
}

#[test]
fn strings_that_do_not_parse_stay_text() {
    assert_eq!(
        json_to_cbor_with_hint(&json!("abc"), Some(&int(0))),
        text("abc")
    );
    assert_eq!(
        json_to_cbor_with_hint(&json!("abc"), Some(&CborValue::Float(0.0))),
        text("abc")
    );
    assert_eq!(
        json_to_cbor_with_hint(&json!("maybe"), Some(&CborValue::Bool(false))),
        text("maybe")
    );
}

#[test]
fn integer_valued_number_under_float_hint_stays_a_float() {
    assert_eq!(
        json_to_cbor_with_hint(&json!(3), Some(&CborValue::Float(0.0))),
        CborValue::Float(3.0)
    );
}

#[test]
fn unrelated_hint_falls_back_to_lossless() {
    assert_eq!(json_to_cbor_with_hint(&json!(5), Some(&text("x"))), int(5));
    assert_eq!(
        json_to_cbor_with_hint(&json!({"a": [1]}), Some(&CborValue::Null)),
        json_to_cbor(json!({"a": [1]}))
    );
}

// epoch pair helpers

#[test]
fn epoch_pair_renders_to_rfc3339_utc() {
    let CborValue::Tag(_, inner) = epoch_pair(1_000_000_000, 5) else {
        unreachable!()
    };
    let s = tag12_to_rfc3339(&inner).unwrap();
    assert!(s.starts_with("2001-09-09T01:46:40"), "{s}");
    assert!(s.contains(".000000005"), "{s}");
    assert!(s.ends_with('Z'), "{s}");
}

#[test]
fn epoch_zero_and_before_render() {
    let zero = CborValue::Array(vec![int(0), int(0)]);
    assert!(
        tag12_to_rfc3339(&zero)
            .unwrap()
            .starts_with("1970-01-01T00:00:00")
    );
    let before = CborValue::Array(vec![int(-1), int(0)]);
    assert!(
        tag12_to_rfc3339(&before)
            .unwrap()
            .starts_with("1969-12-31T23:59:59")
    );
}

#[test]
fn one_element_epoch_pair_means_zero_nanos() {
    let one = CborValue::Array(vec![int(1_000_000_000)]);
    let two = CborValue::Array(vec![int(1_000_000_000), int(0)]);
    assert_eq!(tag12_to_rfc3339(&one), tag12_to_rfc3339(&two));
    assert!(tag12_to_rfc3339(&one).is_some());
}

#[test]
fn malformed_epoch_pairs_render_nothing() {
    assert_eq!(tag12_to_rfc3339(&CborValue::Array(vec![])), None);
    assert_eq!(
        tag12_to_rfc3339(&CborValue::Array(vec![int(1), int(2), int(3)])),
        None
    );
    assert_eq!(
        tag12_to_rfc3339(&CborValue::Array(vec![int(1), CborValue::Float(0.5)])),
        None
    );
    assert_eq!(
        tag12_to_rfc3339(&CborValue::Array(vec![CborValue::Float(1.0)])),
        None
    );
    assert_eq!(tag12_to_rfc3339(&text("x")), None);
}

#[test]
fn rfc3339_parses_back_to_an_epoch_pair() {
    assert_eq!(
        rfc3339_to_tag12("2001-09-09T01:46:40.000000005Z"),
        Some(epoch_pair(1_000_000_000, 5))
    );
}

#[test]
fn rfc3339_offsets_are_normalised_to_utc() {
    assert_eq!(
        rfc3339_to_tag12("2001-09-09T03:46:40+02:00"),
        Some(epoch_pair(1_000_000_000, 0))
    );
}

#[test]
fn rfc3339_rejects_non_dates() {
    assert_eq!(rfc3339_to_tag12("yesterday"), None);
    assert_eq!(rfc3339_to_tag12(""), None);
}

#[test]
fn epoch_pair_and_rfc3339_are_inverse() {
    let CborValue::Tag(_, inner) = epoch_pair(1_816_421_400, 123_456_789) else {
        unreachable!()
    };
    let s = tag12_to_rfc3339(&inner).unwrap();
    assert_eq!(
        rfc3339_to_tag12(&s),
        Some(epoch_pair(1_816_421_400, 123_456_789))
    );
}

// cbor_id_to_string

#[test]
fn ids_stringify_by_shape() {
    assert_eq!(
        cbor_id_to_string(&record_id("bakery", text("1"))).as_deref(),
        Some("bakery:1")
    );
    assert_eq!(cbor_id_to_string(&text("abc")).as_deref(), Some("abc"));
    assert_eq!(cbor_id_to_string(&int(42)).as_deref(), Some("42"));
    assert_eq!(
        cbor_id_to_string(&CborValue::Bool(true)).as_deref(),
        Some("true")
    );
    assert_eq!(
        cbor_id_to_string(&record_id("user", int(7))).as_deref(),
        Some("user:7")
    );
}

#[test]
fn other_tags_are_transparent_for_ids() {
    assert_eq!(
        cbor_id_to_string(&tagged(6, text("x"))).as_deref(),
        Some("x")
    );
    assert_eq!(
        cbor_id_to_string(&tagged(8, text("user:1"))).as_deref(),
        Some("user:1")
    );
}

#[test]
fn shapes_that_cannot_be_an_id_yield_none() {
    for v in [
        CborValue::Null,
        CborValue::Float(1.5),
        CborValue::Bytes(vec![1, 2]),
        CborValue::Array(vec![text("a")]),
        CborValue::Map(vec![]),
        tagged(8, CborValue::Array(vec![text("a"), CborValue::Null])),
    ] {
        assert_eq!(cbor_id_to_string(&v), None, "{v:?} is not an id");
    }
}

#[test]
fn null_has_no_id_but_an_empty_string() {
    assert_eq!(cbor_id_to_string(&CborValue::Null), None);
    assert_eq!(cbor_to_string(&PlainDialect, &CborValue::Null), "");
}

// hex_encode

#[test]
fn hex_encode_is_lowercase_and_zero_padded() {
    assert_eq!(hex_encode(&[]), "");
    assert_eq!(hex_encode(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
}
