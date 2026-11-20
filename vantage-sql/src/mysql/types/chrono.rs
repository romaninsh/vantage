//! Chrono types for MySQL, in the carrier forms of [`vantage_types::temporal`]: an instant is
//! `Tag(0)` UTC text with six fraction digits and `Z`, a date `Tag(1004)`, a time `Tag(101)`.
//! A naive datetime is a UTC wall clock: `DATETIME` holds UTC by convention, and every connection
//! runs with `time_zone = '+00:00'` (sqlx's default), so `TIMESTAMP` doesn't shift either.
//!
//! `from_cbor` accepts every form the lenient readers know, tagged or plain text, so values from
//! untyped (VARCHAR) columns still extract as chrono types.

use super::{MysqlType, MysqlTypeDateMarker, MysqlTypeDateTimeMarker, MysqlTypeTimeMarker};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use ciborium::Value;
use vantage_types::temporal;

impl MysqlType for NaiveDate {
    type Target = MysqlTypeDateMarker;

    fn to_cbor(&self) -> Value {
        temporal::date(self)
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_date(&value)
    }
}

impl MysqlType for NaiveTime {
    type Target = MysqlTypeTimeMarker;

    fn to_cbor(&self) -> Value {
        temporal::time(self)
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_time(&value)
    }
}

impl MysqlType for NaiveDateTime {
    type Target = MysqlTypeDateTimeMarker;

    fn to_cbor(&self) -> Value {
        temporal::instant(&self.and_utc())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_instant(&value).map(|at| at.naive_utc())
    }
}

impl MysqlType for DateTime<Utc> {
    type Target = MysqlTypeDateTimeMarker;

    fn to_cbor(&self) -> Value {
        temporal::instant(self)
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_instant(&value)
    }
}

impl MysqlType for DateTime<FixedOffset> {
    type Target = MysqlTypeDateTimeMarker;

    fn to_cbor(&self) -> Value {
        temporal::instant(&self.with_timezone(&Utc))
    }

    fn from_cbor(value: Value) -> Option<Self> {
        temporal::as_instant(&value).map(|at| at.fixed_offset())
    }
}
