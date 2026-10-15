//! `AnyCsvType`: typed values stored as text with a variant tag, and the
//! conversions around it.

use vantage_csv::type_system::{parse_with_type, variant_from_type_name};
use vantage_csv::{AnyCsvType, CsvTypeVariants, record_to_json};
use vantage_types::{Record, RichText, Style, TerminalRender};

#[test]
fn string_round_trips_with_string_variant() {
    let any = AnyCsvType::new("hello".to_string());

    assert_eq!(any.type_variant(), Some(CsvTypeVariants::String));
    assert_eq!(any.try_get::<String>().as_deref(), Some("hello"));
    assert_eq!(any.value(), "hello");
}

#[test]
fn int_round_trips_and_is_stored_as_decimal_text() {
    let any = AnyCsvType::new(42_i64);

    assert_eq!(any.type_variant(), Some(CsvTypeVariants::Int));
    assert_eq!(any.try_get::<i64>(), Some(42));
    assert_eq!(any.value(), "42");
}

#[test]
fn float_round_trips() {
    let any = AnyCsvType::new(2.5_f64);

    assert_eq!(any.type_variant(), Some(CsvTypeVariants::Float));
    assert_eq!(any.try_get::<f64>(), Some(2.5));
    assert_eq!(any.value(), "2.5");
}

#[test]
fn bool_round_trips_and_is_stored_as_true_or_false() {
    let yes = AnyCsvType::new(true);
    let no = AnyCsvType::new(false);

    assert_eq!(yes.type_variant(), Some(CsvTypeVariants::Bool));
    assert_eq!(yes.try_get::<bool>(), Some(true));
    assert_eq!(yes.value(), "true");
    assert_eq!(no.try_get::<bool>(), Some(false));
    assert_eq!(no.value(), "false");
}

#[test]
fn json_round_trips() {
    let obj = serde_json::json!({"stock": 50});
    let any = AnyCsvType::new(obj.clone());

    assert_eq!(any.type_variant(), Some(CsvTypeVariants::Json));
    assert_eq!(any.try_get::<serde_json::Value>(), Some(obj));
}

#[test]
fn list_round_trips_mixed_element_types() {
    let any = AnyCsvType::new(vec![
        AnyCsvType::new("hello".to_string()),
        AnyCsvType::new(42_i64),
        AnyCsvType::new(true),
    ]);
    assert_eq!(any.type_variant(), Some(CsvTypeVariants::List));

    let items = any.try_get::<Vec<AnyCsvType>>().expect("list");
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].try_get::<String>().as_deref(), Some("hello"));
    assert_eq!(items[1].try_get::<i64>(), Some(42));
    assert_eq!(items[2].try_get::<bool>(), Some(true));
}

#[test]
fn list_round_trips_all_element_variants() {
    let animal = parse_with_type("cat", Some(CsvTypeVariants::Animal));
    let any = AnyCsvType::new(vec![
        AnyCsvType::new(1.5_f64),
        AnyCsvType::new(serde_json::json!({"a": 1})),
        animal,
        AnyCsvType::new(vec![AnyCsvType::new(7_i64)]),
    ]);

    let items = any.try_get::<Vec<AnyCsvType>>().expect("list");
    let variants: Vec<_> = items.iter().map(|i| i.type_variant()).collect();
    assert_eq!(
        variants,
        [
            Some(CsvTypeVariants::Float),
            Some(CsvTypeVariants::Json),
            Some(CsvTypeVariants::Animal),
            Some(CsvTypeVariants::List),
        ]
    );
    assert_eq!(items[0].try_get::<f64>(), Some(1.5));
    assert_eq!(items[2].value(), "cat");
}

#[test]
fn empty_list_round_trips() {
    let any = AnyCsvType::new(Vec::<AnyCsvType>::new());

    assert_eq!(any.type_variant(), Some(CsvTypeVariants::List));
    assert_eq!(any.try_get::<Vec<AnyCsvType>>(), Some(Vec::new()));
}

#[test]
fn option_some_stores_the_inner_value_and_none_stores_empty_text() {
    let some = AnyCsvType::new(Some(42_i64));
    assert_eq!(some.type_variant(), Some(CsvTypeVariants::Int));
    assert_eq!(some.try_get::<Option<i64>>(), Some(Some(42)));

    let none = AnyCsvType::new(None::<i64>);
    assert_eq!(none.value(), "");
    assert_eq!(none.try_get::<Option<i64>>(), Some(None));
}

#[test]
fn parse_with_type_uses_the_column_variant() {
    let int = parse_with_type("300", Some(CsvTypeVariants::Int));
    assert_eq!(int.type_variant(), Some(CsvTypeVariants::Int));
    assert_eq!(int.try_get::<i64>(), Some(300));

    let float = parse_with_type("2.5", Some(CsvTypeVariants::Float));
    assert_eq!(float.type_variant(), Some(CsvTypeVariants::Float));
    assert_eq!(float.try_get::<f64>(), Some(2.5));

    let flag = parse_with_type("true", Some(CsvTypeVariants::Bool));
    assert_eq!(flag.type_variant(), Some(CsvTypeVariants::Bool));
    assert_eq!(flag.try_get::<bool>(), Some(true));

    let json = parse_with_type(r#"{"stock":50}"#, Some(CsvTypeVariants::Json));
    assert_eq!(json.type_variant(), Some(CsvTypeVariants::Json));
    assert_eq!(
        json.try_get::<serde_json::Value>().expect("json")["stock"],
        serde_json::json!(50)
    );

    let animal = parse_with_type("dog", Some(CsvTypeVariants::Animal));
    assert_eq!(animal.type_variant(), Some(CsvTypeVariants::Animal));
    assert_eq!(animal.value(), "dog");
}

#[test]
fn parse_with_type_list_decodes_an_encoded_list() {
    let encoded = AnyCsvType::new(vec![AnyCsvType::new(1_i64), AnyCsvType::new(2_i64)]);

    let parsed = parse_with_type(encoded.value(), Some(CsvTypeVariants::List));

    assert_eq!(parsed.type_variant(), Some(CsvTypeVariants::List));
    let items = parsed.try_get::<Vec<AnyCsvType>>().expect("list");
    assert_eq!(items[0].try_get::<i64>(), Some(1));
    assert_eq!(items[1].try_get::<i64>(), Some(2));
}

#[test]
fn parse_with_type_without_a_variant_is_a_plain_string() {
    for variant in [None, Some(CsvTypeVariants::String)] {
        let any = parse_with_type("42", variant);
        assert_eq!(any.type_variant(), Some(CsvTypeVariants::String));
        assert_eq!(any.try_get::<String>().as_deref(), Some("42"));
    }
}

#[test]
fn parse_with_type_empty_input_is_untyped_null_for_every_variant() {
    for variant in [
        CsvTypeVariants::String,
        CsvTypeVariants::Int,
        CsvTypeVariants::Float,
        CsvTypeVariants::Bool,
        CsvTypeVariants::Json,
    ] {
        let any = parse_with_type("", Some(variant));
        assert_eq!(any.type_variant(), None);
        assert_eq!(any.value(), "");
    }
}

#[test]
fn parse_with_type_unparseable_input_falls_back_to_string() {
    for (raw, variant) in [
        ("abc", CsvTypeVariants::Int),
        ("abc", CsvTypeVariants::Float),
        ("yes", CsvTypeVariants::Bool),
        ("{not json", CsvTypeVariants::Json),
    ] {
        let any = parse_with_type(raw, Some(variant));
        assert_eq!(any.type_variant(), Some(CsvTypeVariants::String), "{raw}");
        assert_eq!(any.value(), raw);
    }
}

#[test]
fn variant_from_type_name_maps_rust_type_names() {
    for name in ["i64", "i32", "u64", "u32"] {
        assert_eq!(variant_from_type_name(name), Some(CsvTypeVariants::Int));
    }
    for name in ["f64", "f32"] {
        assert_eq!(variant_from_type_name(name), Some(CsvTypeVariants::Float));
    }
    assert_eq!(variant_from_type_name("bool"), Some(CsvTypeVariants::Bool));
    assert_eq!(
        variant_from_type_name("alloc::string::String"),
        Some(CsvTypeVariants::String)
    );
    assert_eq!(
        variant_from_type_name("serde_json::value::Value"),
        Some(CsvTypeVariants::Json)
    );
    assert_eq!(variant_from_type_name("some::Unrelated"), None);
}

#[test]
fn display_quotes_text_and_leaves_other_values_as_stored() {
    assert_eq!(
        AnyCsvType::new("Doc Brown".to_string()).to_string(),
        "\"Doc Brown\""
    );
    assert_eq!(AnyCsvType::new(42_i64).to_string(), "42");
    assert_eq!(AnyCsvType::new(true).to_string(), "true");
    assert_eq!(AnyCsvType::new(2.5_f64).to_string(), "2.5");
}

#[test]
fn from_native_scalars_tags_the_variant() {
    assert_eq!(
        AnyCsvType::from(7_i64).type_variant(),
        Some(CsvTypeVariants::Int)
    );
    assert_eq!(
        AnyCsvType::from(0.5_f64).type_variant(),
        Some(CsvTypeVariants::Float)
    );
    assert_eq!(
        AnyCsvType::from(false).type_variant(),
        Some(CsvTypeVariants::Bool)
    );
}

#[test]
fn from_json_maps_each_json_kind_to_a_variant() {
    let null = AnyCsvType::from(serde_json::Value::Null);
    assert_eq!(null.type_variant(), None);
    assert_eq!(null.value(), "");

    let flag = AnyCsvType::from(serde_json::json!(true));
    assert_eq!(flag.try_get::<bool>(), Some(true));

    let int = AnyCsvType::from(serde_json::json!(12));
    assert_eq!(int.type_variant(), Some(CsvTypeVariants::Int));
    assert_eq!(int.try_get::<i64>(), Some(12));

    let float = AnyCsvType::from(serde_json::json!(1.25));
    assert_eq!(float.type_variant(), Some(CsvTypeVariants::Float));
    assert_eq!(float.try_get::<f64>(), Some(1.25));

    let text = AnyCsvType::from(serde_json::json!("hi"));
    assert_eq!(text.type_variant(), Some(CsvTypeVariants::String));
    assert_eq!(text.value(), "hi");
}

#[test]
fn into_json_maps_each_variant_to_its_json_kind() {
    let json = |v: AnyCsvType| serde_json::Value::from(v);

    assert_eq!(json(AnyCsvType::new(5_i64)), serde_json::json!(5));
    assert_eq!(json(AnyCsvType::new(1.5_f64)), serde_json::json!(1.5));
    assert_eq!(json(AnyCsvType::new(true)), serde_json::json!(true));
    assert_eq!(
        json(AnyCsvType::new(serde_json::json!({"a": [1, 2]}))),
        serde_json::json!({"a": [1, 2]})
    );
    assert_eq!(
        json(AnyCsvType::new("text".to_string())),
        serde_json::json!("text")
    );
    assert_eq!(
        json(parse_with_type("cat", Some(CsvTypeVariants::Animal))),
        serde_json::json!("cat")
    );
    assert_eq!(
        json(AnyCsvType::new(vec![
            AnyCsvType::new(1_i64),
            AnyCsvType::new("x".to_string())
        ])),
        serde_json::json!([1, "x"])
    );
}

#[test]
fn untyped_empty_value_converts_to_json_null() {
    let empty = parse_with_type("", None);
    assert_eq!(serde_json::Value::from(empty), serde_json::Value::Null);
}

#[test]
fn record_to_json_converts_every_field() {
    let mut record = Record::new();
    record.insert("name".to_string(), AnyCsvType::new("Doc".to_string()));
    record.insert("age".to_string(), AnyCsvType::new(65_i64));

    let json = record_to_json(record);

    assert_eq!(json["name"], serde_json::json!("Doc"));
    assert_eq!(json["age"], serde_json::json!(65));
}

#[test]
fn cbor_conversion_round_trips_scalars() {
    use ciborium::Value as Cbor;

    let int = AnyCsvType::from(Cbor::Integer(9.into()));
    assert_eq!(int.try_get::<i64>(), Some(9));
    assert_eq!(Cbor::from(int), Cbor::Integer(9.into()));

    let flag = AnyCsvType::from(Cbor::Bool(true));
    assert_eq!(flag.try_get::<bool>(), Some(true));
    assert_eq!(Cbor::from(flag), Cbor::Bool(true));

    let text = AnyCsvType::from(Cbor::Text("hello".to_string()));
    assert_eq!(text.value(), "hello");
    assert_eq!(Cbor::from(text), Cbor::Text("hello".to_string()));
}

#[test]
fn render_styles_booleans_and_marks_null_as_muted() {
    assert_eq!(
        AnyCsvType::new(true).render(),
        RichText::styled("true", Style::Success)
    );
    assert_eq!(
        AnyCsvType::new(false).render(),
        RichText::styled("false", Style::Error)
    );
    assert_eq!(
        parse_with_type("", None).render(),
        RichText::styled("—", Style::Muted)
    );
    assert_eq!(
        AnyCsvType::new("plain".to_string()).render(),
        RichText::plain("plain")
    );
}

#[test]
fn render_shows_animals_as_glyphs() {
    let animal = |name: &str| parse_with_type(name, Some(CsvTypeVariants::Animal)).render();

    assert_eq!(animal("cat"), RichText::plain("🐱"));
    assert_eq!(animal("dog"), RichText::plain("🐶"));
    assert_eq!(animal("pig"), RichText::plain("🐷"));
    assert_eq!(animal("cow"), RichText::plain("🐮"));
    assert_eq!(animal("chicken"), RichText::plain("🐔"));
    assert_eq!(animal("dodo"), RichText::plain("dodo"));
}
