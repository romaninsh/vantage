use ciborium::Value as CborValue;
use vantage_types::{
    IntoRecord, InvariantValue, Record, TryFromRecord, entity, vantage_type_system,
};

vantage_type_system! {
    type_trait: Demo,
    method_name: cbor,
    value_type: CborValue,
    null_when: CborValue::Null,
    type_variants: [Text, Int, Stamp]
}

impl DemoVariants {
    pub fn from_cbor(value: &CborValue) -> Option<Self> {
        match value {
            CborValue::Text(_) => Some(DemoVariants::Text),
            CborValue::Integer(_) => Some(DemoVariants::Int),
            CborValue::Tag(1, _) => Some(DemoVariants::Stamp),
            _ => None,
        }
    }
}

impl Demo for String {
    type Target = DemoTextMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Text(self.clone())
    }

    fn from_cbor(value: CborValue) -> Option<Self> {
        match value {
            CborValue::Text(s) => Some(s),
            _ => None,
        }
    }
}

impl Demo for i64 {
    type Target = DemoIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(value: CborValue) -> Option<Self> {
        match value {
            CborValue::Integer(i) => i64::try_from(i128::from(i)).ok(),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
struct Stamp(String);

impl Demo for Stamp {
    type Target = DemoStampMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Tag(1, Box::new(CborValue::Text(self.0.clone())))
    }

    fn from_cbor(value: CborValue) -> Option<Self> {
        match value {
            CborValue::Tag(1, inner) => match *inner {
                CborValue::Text(s) => Some(Stamp(s)),
                _ => None,
            },
            _ => None,
        }
    }
}

// A value whose shape the variant detector does not recognise.
impl Demo for bool {
    type Target = DemoIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Bool(*self)
    }

    fn from_cbor(value: CborValue) -> Option<Self> {
        match value {
            CborValue::Bool(b) => Some(b),
            _ => None,
        }
    }
}

// No `null_when`: a non-nullable representation.
vantage_type_system! {
    type_trait: Plain,
    method_name: plain,
    value_type: String,
    type_variants: [Word]
}

impl PlainVariants {
    pub fn from_plain(_value: &String) -> Option<Self> {
        Some(PlainVariants::Word)
    }
}

impl Plain for String {
    type Target = PlainWordMarker;

    fn to_plain(&self) -> String {
        self.clone()
    }

    fn from_plain(value: String) -> Option<Self> {
        Some(value)
    }
}

#[test]
fn new_stores_the_value_and_records_the_variant() {
    let text = AnyDemo::new("hello".to_string());
    assert_eq!(text.value(), &CborValue::Text("hello".into()));
    assert_eq!(text.type_variant(), Some(DemoVariants::Text));

    let int = AnyDemo::new(42i64);
    assert_eq!(int.value(), &CborValue::Integer(42.into()));
    assert_eq!(int.type_variant(), Some(DemoVariants::Int));

    let stamp = AnyDemo::new(Stamp("now".into()));
    assert_eq!(stamp.type_variant(), Some(DemoVariants::Stamp));
}

#[test]
fn new_ref_matches_new_without_consuming() {
    let s = "hello".to_string();
    let by_ref = AnyDemo::new_ref(&s);
    assert_eq!(by_ref, AnyDemo::new(s.clone()));
    assert_eq!(by_ref.type_variant(), Some(DemoVariants::Text));
    assert_eq!(s, "hello");
}

#[test]
fn try_get_returns_the_value_for_the_same_variant() {
    let int = AnyDemo::new(42i64);
    assert_eq!(int.try_get::<i64>(), Some(42));

    let text = AnyDemo::new("hello".to_string());
    assert_eq!(text.try_get::<String>(), Some("hello".to_string()));

    let stamp = AnyDemo::new(Stamp("now".into()));
    assert_eq!(stamp.try_get::<Stamp>(), Some(Stamp("now".into())));
}

#[test]
fn try_get_rejects_a_different_variant() {
    let int = AnyDemo::new(42i64);
    assert_eq!(int.try_get::<String>(), None);

    let text = AnyDemo::new("42".to_string());
    assert_eq!(text.try_get::<i64>(), None);
    assert_eq!(text.try_get::<Stamp>(), None);
}

#[test]
fn from_cbor_infers_the_variant_from_the_stored_value() {
    let any = AnyDemo::from_cbor(&CborValue::Integer(7.into())).unwrap();
    assert_eq!(any.type_variant(), Some(DemoVariants::Int));
    assert_eq!(any.try_get::<i64>(), Some(7));
    assert_eq!(any.try_get::<String>(), None);

    let any =
        AnyDemo::from_cbor(&CborValue::Tag(1, Box::new(CborValue::Text("t".into())))).unwrap();
    assert_eq!(any.type_variant(), Some(DemoVariants::Stamp));
}

#[test]
fn unrecognised_values_keep_the_value_but_no_variant() {
    let any = AnyDemo::from_cbor(&CborValue::Bool(true)).unwrap();
    assert_eq!(any.type_variant(), None);
    assert_eq!(any.value(), &CborValue::Bool(true));
    assert_eq!(any.try_get::<bool>(), Some(true));
    assert_eq!(any.try_get::<String>(), None);
}

#[test]
fn conversions_with_the_value_type_round_trip() {
    let any: AnyDemo = CborValue::Text("hi".into()).into();
    assert_eq!(any.type_variant(), Some(DemoVariants::Text));

    let value: CborValue = any.clone().into();
    assert_eq!(value, CborValue::Text("hi".into()));
    assert_eq!(any.into_value(), CborValue::Text("hi".into()));
}

#[test]
fn equality_compares_the_stored_value_only() {
    let built = AnyDemo::new(true);
    let read_back = AnyDemo::from(CborValue::Bool(true));
    assert_eq!(built.type_variant(), Some(DemoVariants::Int));
    assert_eq!(read_back.type_variant(), None);
    assert_eq!(built, read_back);

    assert_ne!(AnyDemo::new(1i64), AnyDemo::new(2i64));
}

#[test]
fn null_when_decides_which_values_are_null() {
    assert!(AnyDemo::from(CborValue::Null).is_null());
    assert!(!AnyDemo::new(0i64).is_null());
    assert!(!AnyDemo::new(String::new()).is_null());
}

#[test]
fn without_null_when_nothing_is_null() {
    assert!(!AnyPlain::new(String::new()).is_null());
    assert!(!AnyPlain::new("x".to_string()).is_null());
}

#[test]
fn value_eq_compares_the_stored_representation() {
    assert!(AnyDemo::new(1i64).value_eq(&AnyDemo::new(1i64)));
    assert!(!AnyDemo::new(1i64).value_eq(&AnyDemo::new(2i64)));
    assert!(AnyPlain::new("a".to_string()).value_eq(&AnyPlain::new("a".to_string())));
    assert!(!AnyPlain::new("a".to_string()).value_eq(&AnyPlain::new("b".to_string())));
}

#[derive(Debug, PartialEq, Clone)]
#[entity(Demo)]
struct Item {
    name: String,
    count: i64,
    created: Stamp,
}

fn item() -> Item {
    Item {
        name: "widget".into(),
        count: 3,
        created: Stamp("now".into()),
    }
}

#[test]
fn entity_converts_each_field_through_its_type() {
    let record: Record<AnyDemo> = item().into_record();

    assert_eq!(record.len(), 3);
    assert_eq!(record["name"].type_variant(), Some(DemoVariants::Text));
    assert_eq!(record["count"].type_variant(), Some(DemoVariants::Int));
    assert_eq!(record["created"].type_variant(), Some(DemoVariants::Stamp));
    let keys: Vec<&str> = record.keys().map(String::as_str).collect();
    assert_eq!(keys, ["name", "count", "created"]);
}

#[test]
fn entity_round_trips_through_its_record() {
    let record: Record<AnyDemo> = item().into_record();
    assert_eq!(Item::from_record(record).unwrap(), item());
}

#[test]
fn entity_round_trips_through_raw_stored_values() {
    let record: Record<AnyDemo> = item().into_record();
    let raw: Record<CborValue> = record.into_record();
    let reread: Record<AnyDemo> = raw.into_record();
    assert_eq!(Item::from_record(reread).unwrap(), item());
}

#[test]
fn entity_rejects_a_field_of_the_wrong_type() {
    let mut record: Record<AnyDemo> = item().into_record();
    record.insert("count".to_string(), AnyDemo::new("three".to_string()));
    assert!(Item::from_record(record).is_err());
}

#[test]
fn entity_rejects_a_record_missing_a_field() {
    let mut record: Record<AnyDemo> = item().into_record();
    record.shift_remove("count");
    assert!(Item::from_record(record).is_err());
}
