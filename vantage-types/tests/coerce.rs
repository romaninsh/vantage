use ciborium::Value as CborValue;
use vantage_types::vantage_type_system;

// Strict decoders on purpose: each type accepts only its own wire shape, so
// anything these tests accept comes from the lenient `coerce_wire`.
vantage_type_system! {
    type_trait: Wire,
    method_name: wire,
    value_type: CborValue,
    null_when: CborValue::Null,
    type_variants: [Text, Int, Float, Bool, Day, Clock, Instant, Money]
}

impl WireVariants {
    pub fn from_wire(_value: &CborValue) -> Option<Self> {
        None
    }
}

fn tagged(tag: u64, value: CborValue) -> Option<String> {
    match value {
        CborValue::Tag(t, inner) if t == tag => match *inner {
            CborValue::Text(s) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

fn tag(tag: u64, s: &str) -> CborValue {
    CborValue::Tag(tag, Box::new(CborValue::Text(s.into())))
}

impl Wire for String {
    type Target = WireTextMarker;
    fn to_wire(&self) -> CborValue {
        CborValue::Text(self.clone())
    }
    fn from_wire(value: CborValue) -> Option<Self> {
        match value {
            CborValue::Text(s) => Some(s),
            _ => None,
        }
    }
}

impl Wire for i64 {
    type Target = WireIntMarker;
    fn to_wire(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }
    fn from_wire(value: CborValue) -> Option<Self> {
        match value {
            CborValue::Integer(i) => i64::try_from(i).ok(),
            _ => None,
        }
    }
}

impl Wire for f64 {
    type Target = WireFloatMarker;
    fn to_wire(&self) -> CborValue {
        CborValue::Float(*self)
    }
    fn from_wire(value: CborValue) -> Option<Self> {
        match value {
            CborValue::Float(f) => Some(f),
            _ => None,
        }
    }
}

impl Wire for bool {
    type Target = WireBoolMarker;
    fn to_wire(&self) -> CborValue {
        CborValue::Bool(*self)
    }
    fn from_wire(value: CborValue) -> Option<Self> {
        match value {
            CborValue::Bool(b) => Some(b),
            _ => None,
        }
    }
}

/// A date the way the SQL drivers encode `NaiveDate`: `Tag(100, "YYYY-MM-DD")`.
#[derive(Debug, PartialEq)]
struct Day(String);

impl Wire for Day {
    type Target = WireDayMarker;
    fn to_wire(&self) -> CborValue {
        tag(100, &self.0)
    }
    fn from_wire(value: CborValue) -> Option<Self> {
        tagged(100, value).map(Day)
    }
}

/// A time of day, `Tag(101, "HH:MM:SS")`.
#[derive(Debug, PartialEq)]
struct Clock(String);

impl Wire for Clock {
    type Target = WireClockMarker;
    fn to_wire(&self) -> CborValue {
        tag(101, &self.0)
    }
    fn from_wire(value: CborValue) -> Option<Self> {
        tagged(101, value).map(Clock)
    }
}

/// An instant, `Tag(0, RFC 3339 text)`.
#[derive(Debug, PartialEq)]
struct Instant(String);

impl Wire for Instant {
    type Target = WireInstantMarker;
    fn to_wire(&self) -> CborValue {
        tag(0, &self.0)
    }
    fn from_wire(value: CborValue) -> Option<Self> {
        tagged(0, value).map(Instant)
    }
}

/// A decimal, `Tag(10, text)`, keeping the text as written.
#[derive(Debug, PartialEq)]
struct Money(String);

impl Wire for Money {
    type Target = WireMoneyMarker;
    fn to_wire(&self) -> CborValue {
        tag(10, &self.0)
    }
    fn from_wire(value: CborValue) -> Option<Self> {
        tagged(10, value).map(Money)
    }
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn int(i: i64) -> CborValue {
    CborValue::Integer(i.into())
}

#[test]
fn from_x_stays_strict() {
    assert_eq!(i64::from_wire(text("42")), None);
    assert_eq!(f64::from_wire(int(5)), None);
    assert_eq!(Day::from_wire(text("2026-12-10")), None);
    assert_eq!(AnyWire::from(text("42")).try_get::<i64>(), None);
    assert_eq!(AnyWire::from(int(5)).try_get::<f64>(), None);
}

#[test]
fn coerce_keeps_values_the_strict_decode_accepts() {
    assert_eq!(i64::coerce_wire(int(7)), Some(7));
    assert_eq!(String::coerce_wire(text("42")), Some("42".into()));
    assert_eq!(
        Day::coerce_wire(tag(100, "2026-12-10")),
        Some(Day("2026-12-10".into()))
    );
}

#[test]
fn coerce_text_to_int() {
    assert_eq!(i64::coerce_wire(text("42")), Some(42));
    assert_eq!(i64::coerce_wire(text("-3")), Some(-3));
    assert_eq!(i64::coerce_wire(text("4.5")), None);
    assert_eq!(i64::coerce_wire(text("forty")), None);
}

#[test]
fn coerce_text_to_float() {
    assert_eq!(f64::coerce_wire(text("2.5")), Some(2.5));
    assert_eq!(f64::coerce_wire(text("42")), Some(42.0));
    assert_eq!(f64::coerce_wire(text("NaN")), None);
    assert_eq!(f64::coerce_wire(text("inf")), None);
}

#[test]
fn coerce_text_to_bool() {
    assert_eq!(bool::coerce_wire(text("true")), Some(true));
    assert_eq!(bool::coerce_wire(text("false")), Some(false));
    assert_eq!(bool::coerce_wire(text("yes")), None);
}

#[test]
fn coerce_int_to_float() {
    assert_eq!(f64::coerce_wire(int(5)), Some(5.0));
    assert_eq!(f64::coerce_wire(int(i64::MAX)), None);
}

#[test]
fn coerce_integral_float_to_int() {
    assert_eq!(i64::coerce_wire(CborValue::Float(3.0)), Some(3));
    assert_eq!(i64::coerce_wire(CborValue::Float(3.5)), None);
    assert_eq!(i64::coerce_wire(CborValue::Float(1e300)), None);
}

#[test]
fn coerce_number_to_decimal_keeps_its_text() {
    assert_eq!(
        Money::coerce_wire(text("12.50")),
        Some(Money("12.50".into()))
    );
    assert_eq!(Money::coerce_wire(int(12)), Some(Money("12".into())));
    assert_eq!(
        Money::coerce_wire(CborValue::Float(10.5)),
        Some(Money("10.5".into()))
    );
    assert_eq!(Money::coerce_wire(text("lots")), None);
    assert_eq!(f64::coerce_wire(tag(10, "12.5")), Some(12.5));
    assert_eq!(i64::coerce_wire(tag(10, "12")), Some(12));
}

#[test]
fn coerce_date_text_to_date() {
    assert_eq!(
        Day::coerce_wire(text("2026-12-10")),
        Some(Day("2026-12-10".into()))
    );
    assert_eq!(Day::coerce_wire(text("2026-13-10")), None);
    assert_eq!(Day::coerce_wire(text("next week")), None);
}

#[test]
fn coerce_midnight_instant_to_date() {
    assert_eq!(
        Day::coerce_wire(tag(0, "2026-12-10T00:00:00Z")),
        Some(Day("2026-12-10".into()))
    );
    assert_eq!(
        Day::coerce_wire(text("2026-12-10T00:00:00.000Z")),
        Some(Day("2026-12-10".into()))
    );
    assert_eq!(Day::coerce_wire(tag(0, "2026-12-10T09:30:00Z")), None);
}

#[test]
fn coerce_date_text_to_instant_at_midnight_utc() {
    assert_eq!(
        Instant::coerce_wire(text("2026-12-10")),
        Some(Instant("2026-12-10T00:00:00Z".into()))
    );
    assert_eq!(
        Instant::coerce_wire(tag(100, "2026-12-10")),
        Some(Instant("2026-12-10T00:00:00Z".into()))
    );
}

#[test]
fn coerce_rfc3339_text_to_instant() {
    assert_eq!(
        Instant::coerce_wire(text("2026-12-10T09:30:00+02:00")),
        Some(Instant("2026-12-10T09:30:00+02:00".into()))
    );
    assert_eq!(Instant::coerce_wire(text("later")), None);
}

#[test]
fn coerce_tagged_values_to_plain_text() {
    assert_eq!(
        String::coerce_wire(tag(0, "2026-12-10T09:30:00Z")),
        Some("2026-12-10T09:30:00Z".into())
    );
    assert_eq!(
        String::coerce_wire(tag(100, "2026-12-10")),
        Some("2026-12-10".into())
    );
    assert_eq!(String::coerce_wire(tag(10, "1.50")), Some("1.50".into()));
    assert_eq!(String::coerce_wire(int(5)), None);
}

#[test]
fn coerce_time_text_to_time() {
    assert_eq!(
        Clock::coerce_wire(text("09:30:00")),
        Some(Clock("09:30:00".into()))
    );
    assert_eq!(
        Clock::coerce_wire(text("09:30")),
        Some(Clock("09:30".into()))
    );
    assert_eq!(Clock::coerce_wire(text("25:00")), None);
}

#[test]
fn coerce_leaves_null_to_the_driver() {
    assert_eq!(i64::coerce_wire(CborValue::Null), None);
    assert_eq!(String::coerce_wire(CborValue::Null), None);
}

#[cfg(feature = "serde")]
mod with_serde {
    use super::*;
    use serde_json::{Value as JsonValue, json};

    #[test]
    fn coerce_surreal_datetime_to_instant_and_date() {
        // 2026-12-10T00:00:00Z as SurrealDB's Tag(12, [seconds, nanos]).
        let midnight = CborValue::Tag(
            12,
            Box::new(CborValue::Array(vec![int(1_796_860_800), int(0)])),
        );
        assert_eq!(
            Instant::coerce_wire(midnight.clone()),
            Some(Instant("2026-12-10T00:00:00.000000000Z".into()))
        );
        assert_eq!(Day::coerce_wire(midnight), Some(Day("2026-12-10".into())));
    }

    vantage_type_system! {
        type_trait: Json,
        method_name: json,
        value_type: JsonValue,
        type_variants: [Int, Float, Bool]
    }

    impl JsonVariants {
        pub fn from_json(_value: &JsonValue) -> Option<Self> {
            None
        }
    }

    impl Json for i64 {
        type Target = JsonIntMarker;
        fn to_json(&self) -> JsonValue {
            json!(self)
        }
        fn from_json(value: JsonValue) -> Option<Self> {
            value.as_i64()
        }
    }

    impl Json for f64 {
        type Target = JsonFloatMarker;
        fn to_json(&self) -> JsonValue {
            json!(self)
        }
        fn from_json(value: JsonValue) -> Option<Self> {
            match value {
                JsonValue::Number(n) if n.is_f64() => n.as_f64(),
                _ => None,
            }
        }
    }

    impl Json for bool {
        type Target = JsonBoolMarker;
        fn to_json(&self) -> JsonValue {
            json!(self)
        }
        fn from_json(value: JsonValue) -> Option<Self> {
            value.as_bool()
        }
    }

    #[test]
    fn coerce_json_text_and_numbers() {
        assert_eq!(i64::from_json(json!("42")), None);
        assert_eq!(i64::coerce_json(json!("42")), Some(42));
        assert_eq!(i64::coerce_json(json!(3.0)), Some(3));
        assert_eq!(i64::coerce_json(json!(3.5)), None);
        assert_eq!(f64::coerce_json(json!(5)), Some(5.0));
        assert_eq!(f64::coerce_json(json!("2.5")), Some(2.5));
        assert_eq!(bool::coerce_json(json!("true")), Some(true));
    }
}

// A value type with no lenient forms: `coerce_x` is the strict decode.
vantage_type_system! {
    type_trait: Plain,
    method_name: plain,
    value_type: Vec<u8>,
    type_variants: [Number]
}

impl PlainVariants {
    pub fn from_plain(_value: &Vec<u8>) -> Option<Self> {
        None
    }
}

impl Plain for u8 {
    type Target = PlainNumberMarker;
    fn to_plain(&self) -> Vec<u8> {
        vec![*self]
    }
    fn from_plain(value: Vec<u8>) -> Option<Self> {
        match value.as_slice() {
            [b] => Some(*b),
            _ => None,
        }
    }
}

#[test]
fn coerce_without_lenient_forms_is_the_strict_decode() {
    assert_eq!(u8::coerce_plain(vec![4]), Some(4));
    assert_eq!(u8::coerce_plain(vec![4, 2]), None);
}
