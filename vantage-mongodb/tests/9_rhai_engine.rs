//! The Rhai vocabulary for MongoDB: the `mongo::oid`, `mongo::date` and
//! `mongo::doc` constructors, and the `Dynamic` <-> BSON conversions the
//! scripts rely on. No database needed.

#![cfg(feature = "rhai")]

use bson::oid::ObjectId;
use bson::{Binary, Bson, DateTime, Decimal128, doc, spec::BinarySubtype};
use vantage_mongodb::rhai_engine::{
    MongoVocab, bson_to_dynamic, dynamic_to_bson, dynamic_to_document, map_to_document,
    register_mongo_onto,
};
use vantage_rhai::rhai::{Dynamic, Engine, Map};
use vantage_rhai::{Block, Env, Host, Limits};

fn eval(code: &str) -> Result<Dynamic, String> {
    let host = Host::builder(Limits::background())
        .vocab(MongoVocab)
        .build();
    host.compile(&Block::from(code))
        .and_then(|script| script.eval(&Env::new()))
        .map_err(|e| e.to_string())
}

fn bson_of(code: &str) -> Bson {
    dynamic_to_bson(eval(code).unwrap()).unwrap()
}

fn map(pairs: &[(&str, Dynamic)]) -> Dynamic {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert((*k).into(), v.clone());
    }
    Dynamic::from_map(m)
}

// ── Constructors ───────────────────────────────────────────────────────────

#[test]
fn oid_builds_an_object_id_from_24_hex_characters_and_returns_an_object_id_as_is() {
    let oid = ObjectId::new();
    assert_eq!(
        bson_of(&format!(r#"mongo::oid(mongo::oid("{oid}"))"#)),
        Bson::ObjectId(oid)
    );
}

#[test]
fn oid_interpolates_as_its_hex_form() {
    let oid = ObjectId::new();
    let shown = eval(&format!(r#"`${{mongo::oid("{oid}")}}`"#)).unwrap();
    assert_eq!(shown.into_string().unwrap(), oid.to_hex());
}

#[test]
fn oid_with_bad_hex_errors_naming_the_constructor_and_the_value() {
    let err = eval(r#"mongo::oid("nope")"#).unwrap_err();
    assert!(
        err.contains("mongo::oid()") && err.contains("nope"),
        "{err}"
    );
}

#[test]
fn date_builds_a_datetime_from_rfc_3339_text_and_returns_a_datetime_as_is() {
    let expected = DateTime::parse_rfc3339_str("2026-01-02T03:04:05Z").unwrap();
    assert_eq!(
        bson_of(r#"mongo::date(mongo::date("2026-01-02T03:04:05Z"))"#),
        Bson::DateTime(expected)
    );
}

#[test]
fn date_with_unparseable_text_errors_naming_the_constructor_and_the_value() {
    let err = eval(r#"mongo::date("yesterday")"#).unwrap_err();
    assert!(
        err.contains("mongo::date()") && err.contains("yesterday"),
        "{err}"
    );
}

#[test]
fn constructors_exist_only_inside_the_mongo_namespace() {
    for (code, name) in [
        (r#"oid("000000000000000000000001")"#, "oid"),
        (r#"date("2026-01-02T03:04:05Z")"#, "date"),
        ("doc([])", "doc"),
    ] {
        let err = eval(code).unwrap_err();
        assert!(
            err.contains("Function not found") && err.contains(name),
            "{code}: {err}"
        );
    }
}

#[test]
fn doc_keeps_the_order_the_pairs_were_given_in() {
    let Bson::Document(built) =
        bson_of(r#"mongo::doc([["b", 1], ["a", #{x: mongo::oid("000000000000000000000001")}]])"#)
    else {
        panic!("expected a document");
    };
    assert_eq!(built.keys().collect::<Vec<_>>(), vec!["b", "a"]);
    assert_eq!(
        built,
        doc! {
            "b": 1_i64,
            "a": { "x": ObjectId::parse_str("000000000000000000000001").unwrap() },
        }
    );
}

#[test]
fn doc_refuses_elements_that_are_not_key_value_pairs() {
    let err = eval(r#"mongo::doc([["a"]])"#).unwrap_err();
    assert!(
        err.contains("mongo::doc()") && err.contains("pair"),
        "{err}"
    );
    let err = eval(r#"mongo::doc([1])"#).unwrap_err();
    assert!(
        err.contains("mongo::doc()") && err.contains("pair"),
        "{err}"
    );
}

#[test]
fn doc_refuses_non_string_keys() {
    let err = eval(r#"mongo::doc([[1, "a"]])"#).unwrap_err();
    assert!(err.contains("mongo::doc()") && err.contains("key"), "{err}");
}

#[test]
fn doc_refuses_a_repeated_key() {
    let err = eval(r#"mongo::doc([["n", 1], ["dup", 2], ["dup", 3]])"#).unwrap_err();
    assert!(
        err.contains("repeats a key") && err.contains("dup"),
        "{err}"
    );
}

#[test]
fn doc_refuses_a_value_with_no_bson_form() {
    let err = eval(r#"mongo::doc([["f", || 1]])"#).unwrap_err();
    assert!(err.contains("BSON"), "{err}");
}

#[test]
fn a_map_literal_converts_to_a_filter_with_keys_sorted() {
    let filter = bson_of(r#"#{price: #{"$gt": 100}, tags: ["a", "b"], gone: (), b: 1, a: 2}"#);
    let Bson::Document(filter) = filter else {
        panic!("expected a document");
    };
    assert_eq!(
        filter.keys().collect::<Vec<_>>(),
        vec!["a", "b", "gone", "price", "tags"]
    );
    assert_eq!(
        filter,
        doc! {
            "a": 2_i64,
            "b": 1_i64,
            "gone": null,
            "price": { "$gt": 100_i64 },
            "tags": ["a", "b"],
        }
    );
}

#[test]
fn the_vocabulary_registers_onto_a_plain_engine() {
    let mut engine = Engine::new();
    register_mongo_onto(&mut engine);
    let oid = ObjectId::new();
    let got = engine
        .eval::<ObjectId>(&format!(r#"mongo::oid("{oid}")"#))
        .unwrap();
    assert_eq!(got, oid);
    let shown = engine
        .eval::<String>(&format!(r#"`${{mongo::oid("{oid}")}}`"#))
        .unwrap();
    assert_eq!(shown, oid.to_hex());
}

#[test]
fn date_values_print_as_rfc_3339_text() {
    let mut engine = Engine::new();
    register_mongo_onto(&mut engine);
    let at = DateTime::parse_rfc3339_str("2026-01-02T03:04:05Z").unwrap();
    let shown = engine
        .eval::<String>(r#"`${mongo::date("2026-01-02T03:04:05Z")}`"#)
        .unwrap();
    assert_eq!(DateTime::parse_rfc3339_str(&shown).unwrap(), at);
}

// ── Dynamic -> BSON ────────────────────────────────────────────────────────

#[test]
fn script_scalars_convert_to_their_bson_types() {
    assert_eq!(dynamic_to_bson(Dynamic::UNIT).unwrap(), Bson::Null);
    assert_eq!(
        dynamic_to_bson(Dynamic::from(true)).unwrap(),
        Bson::Boolean(true)
    );
    assert_eq!(
        dynamic_to_bson(Dynamic::from(7_i64)).unwrap(),
        Bson::Int64(7)
    );
    assert_eq!(
        dynamic_to_bson(Dynamic::from(1.5_f64)).unwrap(),
        Bson::Double(1.5)
    );
    assert_eq!(
        dynamic_to_bson(Dynamic::from("hi")).unwrap(),
        Bson::String("hi".into())
    );
    assert_eq!(
        dynamic_to_bson(Dynamic::from('x')).unwrap(),
        Bson::String("x".into())
    );
    assert_eq!(
        dynamic_to_bson(Dynamic::from_blob(vec![1, 2])).unwrap(),
        Bson::Binary(Binary {
            subtype: BinarySubtype::Generic,
            bytes: vec![1, 2],
        })
    );
}

#[test]
fn script_arrays_and_maps_convert_with_maps_in_sorted_key_order() {
    let inner = map(&[("z", Dynamic::from(1_i64)), ("a", Dynamic::from(2_i64))]);
    let outer = map(&[
        (
            "list",
            Dynamic::from_array(vec![Dynamic::from(1_i64), inner]),
        ),
        ("$and", Dynamic::UNIT),
    ]);
    let Bson::Document(doc) = dynamic_to_bson(outer).unwrap() else {
        panic!("expected a document");
    };
    assert_eq!(doc.keys().collect::<Vec<_>>(), vec!["$and", "list"]);
    let Some(Bson::Array(list)) = doc.get("list") else {
        panic!("list should be an array");
    };
    assert_eq!(list[0], Bson::Int64(1));
    let Bson::Document(nested) = &list[1] else {
        panic!("nested map should be a document");
    };
    assert_eq!(nested.keys().collect::<Vec<_>>(), vec!["a", "z"]);
}

#[test]
fn typed_bson_values_pass_through_a_script_unchanged() {
    let oid = ObjectId::new();
    assert_eq!(
        dynamic_to_bson(Dynamic::from(oid)).unwrap(),
        Bson::ObjectId(oid)
    );
    let at = DateTime::from_millis(1_700_000_000_000);
    assert_eq!(
        dynamic_to_bson(Dynamic::from(at)).unwrap(),
        Bson::DateTime(at)
    );
    let ordered = doc! { "b": 1, "a": 2 };
    assert_eq!(
        dynamic_to_bson(Dynamic::from(ordered.clone())).unwrap(),
        Bson::Document(ordered)
    );
    let amount = Bson::Decimal128(Decimal128::from_bytes([1; 16]));
    assert_eq!(
        dynamic_to_bson(Dynamic::from(amount.clone())).unwrap(),
        amount
    );
}

#[test]
fn a_value_with_no_bson_form_is_an_error_naming_its_type() {
    #[derive(Clone)]
    struct Opaque;
    let err = dynamic_to_bson(Dynamic::from(Opaque)).unwrap_err();
    assert!(err.to_string().contains("Opaque"), "{err}");

    let nested = map(&[("bad", Dynamic::from(Opaque))]);
    assert!(dynamic_to_bson(nested).is_err());
    let list = Dynamic::from_array(vec![Dynamic::from(Opaque)]);
    assert!(dynamic_to_bson(list).is_err());
}

#[test]
fn map_to_document_follows_the_maps_sorted_key_order() {
    let mut m = Map::new();
    m.insert("b".into(), Dynamic::from(1_i64));
    m.insert("a".into(), Dynamic::from(2_i64));
    let document = map_to_document(m).unwrap();
    assert_eq!(document.keys().collect::<Vec<_>>(), vec!["a", "b"]);
}

#[test]
fn dynamic_to_document_takes_maps_and_documents_and_refuses_the_rest() {
    let from_map = dynamic_to_document(map(&[("a", Dynamic::from(1_i64))])).unwrap();
    assert_eq!(from_map, doc! { "a": 1_i64 });
    let from_doc = dynamic_to_document(Dynamic::from(doc! { "b": 2, "a": 1 })).unwrap();
    assert_eq!(from_doc.keys().collect::<Vec<_>>(), vec!["b", "a"]);
    let err = dynamic_to_document(Dynamic::from(1_i64)).unwrap_err();
    assert!(err.to_string().contains("expected a map"), "{err}");
}

// ── BSON -> Dynamic ────────────────────────────────────────────────────────

#[test]
fn bson_scalars_become_native_script_values() {
    assert!(bson_to_dynamic(&Bson::Null).is_unit());
    assert!(bson_to_dynamic(&Bson::Undefined).is_unit());
    assert_eq!(bson_to_dynamic(&Bson::Boolean(true)).as_bool(), Ok(true));
    assert_eq!(bson_to_dynamic(&Bson::Int64(9)).as_int(), Ok(9));
    assert_eq!(bson_to_dynamic(&Bson::Double(0.5)).as_float(), Ok(0.5));
    assert_eq!(
        bson_to_dynamic(&Bson::String("s".into()))
            .into_string()
            .unwrap(),
        "s"
    );
}

#[test]
fn an_int32_widens_to_the_scripts_only_integer_type() {
    assert_eq!(bson_to_dynamic(&Bson::Int32(2)).as_int(), Ok(2));
}

#[test]
fn bson_documents_and_arrays_become_maps_and_arrays() {
    let value = bson_to_dynamic(&Bson::Document(doc! { "k": [1_i64, "x"] }));
    let map = value.cast::<Map>();
    let list = map["k"].clone().into_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].as_int(), Ok(1));
}

#[test]
fn bson_object_ids_and_dates_stay_typed_and_generic_binary_becomes_a_blob() {
    let oid = ObjectId::new();
    assert_eq!(
        bson_to_dynamic(&Bson::ObjectId(oid)).cast::<ObjectId>(),
        oid
    );
    let at = DateTime::from_millis(1_700_000_000_000);
    assert_eq!(bson_to_dynamic(&Bson::DateTime(at)).cast::<DateTime>(), at);
    let blob = bson_to_dynamic(&Bson::Binary(Binary {
        subtype: BinarySubtype::Generic,
        bytes: vec![3, 4],
    }));
    assert_eq!(blob.into_blob().unwrap(), vec![3, 4]);
}

#[test]
fn bson_without_a_native_form_is_carried_as_the_bson_itself() {
    let amount = Bson::Decimal128(Decimal128::from_bytes([1; 16]));
    assert_eq!(bson_to_dynamic(&amount).cast::<Bson>(), amount);
    let ts = Bson::Timestamp(bson::Timestamp {
        time: 1,
        increment: 2,
    });
    assert_eq!(bson_to_dynamic(&ts).cast::<Bson>(), ts);
}

#[test]
fn a_document_round_trips_through_a_script_value() {
    let oid = ObjectId::new();
    let at = DateTime::from_millis(1_700_000_000_000);
    let original = doc! {
        "n": 1_i64,
        "small": 2_i32,
        "f": 0.5,
        "s": "text",
        "ok": false,
        "none": null,
        "id": oid,
        "at": at,
        "list": [1_i64, "x", { "k": "v" }],
        "dec": Bson::Decimal128(Decimal128::from_bytes([1; 16])),
    };
    let back = dynamic_to_bson(bson_to_dynamic(&Bson::Document(original.clone()))).unwrap();
    let Bson::Document(back) = back else {
        panic!("expected a document");
    };
    // The only change is the Int32 widening to the script's integer type.
    assert_eq!(back.get("small"), Some(&Bson::Int64(2)));
    for key in ["n", "f", "s", "ok", "none", "id", "at", "list", "dec"] {
        assert_eq!(back.get(key), original.get(key), "{key}");
    }
}
