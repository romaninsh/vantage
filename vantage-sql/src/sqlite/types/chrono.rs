//! Chrono types for SQLite, in the carrier forms of [`vantage_types::temporal`]: an instant is
//! `Tag(0)` UTC text with six fraction digits and `Z`, a date `Tag(1004)`, a time `Tag(101)`.
//! A naive datetime is a UTC wall clock.
//!
//! SQLite has no temporal type, so a column keeps the carrier's text: fixed width, so text
//! order is time order in conditions and `ORDER BY`.
//!
//! `from_cbor` accepts every form the lenient readers know, tagged or plain text: a row read
//! through a table comes back tagged for the columns declared with one of these types, and as
//! untagged text otherwise (a raw query, an undeclared column).

use super::{SqliteType, SqliteTypeTextMarker};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use ciborium::Value;
use vantage_types::temporal;

impl SqliteType for NaiveDate {
    type Target = SqliteTypeTextMarker;

    fn to_cbor(&self) -> Value {
        temporal::date(self)
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_date(&value)
    }
}

impl SqliteType for NaiveTime {
    type Target = SqliteTypeTextMarker;

    fn to_cbor(&self) -> Value {
        temporal::time(self)
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_time(&value)
    }
}

impl SqliteType for NaiveDateTime {
    type Target = SqliteTypeTextMarker;

    fn to_cbor(&self) -> Value {
        temporal::instant(&self.and_utc())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_instant(&value).map(|at| at.naive_utc())
    }
}

impl SqliteType for DateTime<Utc> {
    type Target = SqliteTypeTextMarker;

    fn to_cbor(&self) -> Value {
        temporal::instant(self)
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_instant(&value)
    }
}

impl SqliteType for DateTime<FixedOffset> {
    type Target = SqliteTypeTextMarker;

    fn to_cbor(&self) -> Value {
        temporal::instant(&self.with_timezone(&Utc))
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_instant(&value).map(|at| at.fixed_offset())
    }
}
