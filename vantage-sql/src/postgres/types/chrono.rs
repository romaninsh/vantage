//! Chrono types for PostgreSQL, in the carrier forms of [`vantage_types::temporal`]: an instant
//! is `Tag(0)` UTC text with six fraction digits and `Z`, a date `Tag(1004)`, a time `Tag(101)`.
//! A naive datetime is a UTC wall clock (`timestamp without time zone` holds UTC by convention).
//!
//! `from_cbor` accepts every form the lenient readers know, tagged or plain text, so values from
//! untyped (VARCHAR) columns still extract as chrono types.

use super::{
    PostgresType, PostgresTypeDateMarker, PostgresTypeDateTimeMarker, PostgresTypeTimeMarker,
};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use ciborium::Value;
use vantage_types::temporal;

impl PostgresType for NaiveDate {
    type Target = PostgresTypeDateMarker;

    fn to_cbor(&self) -> Value {
        temporal::date(self)
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_date(&value)
    }
}

impl PostgresType for NaiveTime {
    type Target = PostgresTypeTimeMarker;

    fn to_cbor(&self) -> Value {
        temporal::time(self)
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_time(&value)
    }
}

impl PostgresType for NaiveDateTime {
    type Target = PostgresTypeDateTimeMarker;

    fn to_cbor(&self) -> Value {
        temporal::instant(&self.and_utc())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_instant(&value).map(|at| at.naive_utc())
    }
}

impl PostgresType for DateTime<Utc> {
    type Target = PostgresTypeDateTimeMarker;

    fn to_cbor(&self) -> Value {
        temporal::instant(self)
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_instant(&value)
    }
}

impl PostgresType for DateTime<FixedOffset> {
    type Target = PostgresTypeDateTimeMarker;

    fn to_cbor(&self) -> Value {
        temporal::instant(&self.with_timezone(&Utc))
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_instant(&value).map(|at| at.fixed_offset())
    }
}
