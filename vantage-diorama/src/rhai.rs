//! Rhai scripting surface over [`Servo`] — the same treatment
//! vantage-vista's `rhai` feature gives queries.
//!
//! [`register_servo_onto`] teaches an engine the `Servo` type; the host
//! pushes an `Arc<Servo>` into scope (or returns one from another
//! registered fn) and scripts drive the draft with the same vocabulary
//! the Rust API has:
//!
//! ```rhai
//! servo.set("name", "Coffee");
//! if servo.is_dirty() {
//!     let id = servo.save();       // flash + settle; the record's id
//! }
//! ```
//!
//! Rhai is synchronous; [`Servo::flash`] is async. `save()` runs the
//! flash via `Handle::current().block_on(…)`, which is only legal on a
//! thread with a runtime *context* but no async *frame* — the same
//! contract as vantage-vista's fetch verbs (see
//! `vantage-vista/src/rhai/runtime.rs`): evaluate scripts under
//! `tokio::task::spawn_blocking`, never directly inside an async task.

use std::sync::Arc;

use ciborium::Value as CborValue;
use rhai::{Dynamic, Engine, EvalAltResult, Map as RhaiMap};
use vantage_types::Record;

use crate::servo::{Servo, ServoStatus};

/// Teach `engine` the `Servo` type and its methods. The host decides how
/// a script *obtains* a servo — a scope variable, a registered lookup fn
/// — this only covers what a script can do once it holds one.
pub fn register_servo_onto(engine: &mut Engine) {
    engine.register_type_with_name::<Arc<Servo>>("Servo");

    engine.register_fn("get", |servo: &mut Arc<Servo>, field: &str| -> Dynamic {
        servo
            .get(field)
            .map(|v| cbor_to_dynamic(&v))
            .unwrap_or(Dynamic::UNIT)
    });

    engine.register_fn(
        "set",
        |servo: &mut Arc<Servo>,
         field: &str,
         value: Dynamic|
         -> Result<(), Box<EvalAltResult>> {
            servo.set(field, dynamic_to_cbor(&value)?);
            Ok(())
        },
    );

    engine.register_fn("id", |servo: &mut Arc<Servo>| -> Dynamic {
        servo.id().map(Dynamic::from).unwrap_or(Dynamic::UNIT)
    });

    engine.register_fn("record", |servo: &mut Arc<Servo>| -> RhaiMap {
        record_to_map(&servo.record())
    });

    engine.register_fn("baseline", |servo: &mut Arc<Servo>| -> Dynamic {
        servo
            .baseline()
            .map(|r| Dynamic::from_map(record_to_map(&r)))
            .unwrap_or(Dynamic::UNIT)
    });

    // The error *signal* (data ≠ baseline), not a failure — same name,
    // same meaning as the Rust API.
    engine.register_fn("error", |servo: &mut Arc<Servo>| -> RhaiMap {
        record_to_map(&servo.error())
    });

    engine.register_fn("dirty", |servo: &mut Arc<Servo>, field: &str| -> bool {
        servo.dirty(field)
    });

    engine.register_fn("is_dirty", |servo: &mut Arc<Servo>| -> bool {
        servo.is_dirty()
    });

    engine.register_fn("revert", |servo: &mut Arc<Servo>, field: &str| {
        servo.revert(field);
    });

    engine.register_fn("revert_all", |servo: &mut Arc<Servo>| {
        servo.revert_all();
    });

    engine.register_fn("status", |servo: &mut Arc<Servo>| -> &'static str {
        match servo.status() {
            ServoStatus::Tracking => "tracking",
            ServoStatus::Pending => "pending",
            ServoStatus::Failed(_) => "failed",
        }
    });

    // The last save's rejection: `()` unless status is failed, else a
    // map of `message` plus per-field errors under `fields`.
    engine.register_fn("rejection", |servo: &mut Arc<Servo>| -> Dynamic {
        match servo.status() {
            ServoStatus::Failed(rejection) => {
                let mut fields = RhaiMap::new();
                for (field, message) in rejection.field_errors() {
                    fields.insert(field.as_str().into(), Dynamic::from(message.clone()));
                }
                let mut map = RhaiMap::new();
                map.insert("message".into(), Dynamic::from(rejection.message().to_string()));
                map.insert("fields".into(), Dynamic::from_map(fields));
                Dynamic::from_map(map)
            }
            _ => Dynamic::UNIT,
        }
    });

    // Actuate: flash the dirty fields and block until the write
    // resolves. Returns the id the record settled under (`()` when
    // nothing was dirty and no id is bound yet); a rejected write is a
    // script error, with the draft surviving on the servo.
    engine.register_fn(
        "save",
        |servo: &mut Arc<Servo>| -> Result<Dynamic, Box<EvalAltResult>> {
            let handle = tokio::runtime::Handle::try_current().map_err(|_| {
                Box::<EvalAltResult>::from(
                    "servo save() needs a tokio runtime context (run the script via spawn_blocking)",
                )
            })?;
            handle
                .block_on(servo.flash())
                .map_err(|e| Box::<EvalAltResult>::from(format!("save failed: {e}")))?;
            Ok(servo.id().map(Dynamic::from).unwrap_or(Dynamic::UNIT))
        },
    );
}

fn record_to_map(record: &Record<CborValue>) -> RhaiMap {
    let mut map = RhaiMap::new();
    for (key, value) in record.iter() {
        map.insert(key.as_str().into(), cbor_to_dynamic(value));
    }
    map
}

/// CBOR → Dynamic, lossy only where Rhai has no representation (a tag is
/// unwrapped to its value, bytes become a blob).
pub fn cbor_to_dynamic(value: &CborValue) -> Dynamic {
    match value {
        CborValue::Null => Dynamic::UNIT,
        CborValue::Bool(b) => Dynamic::from(*b),
        CborValue::Integer(i) => {
            let wide: i128 = (*i).into();
            i64::try_from(wide)
                .map(Dynamic::from)
                .unwrap_or_else(|_| Dynamic::from(wide as f64))
        }
        CborValue::Float(f) => Dynamic::from(*f),
        CborValue::Text(s) => Dynamic::from(s.clone()),
        CborValue::Bytes(b) => Dynamic::from_blob(b.clone()),
        CborValue::Array(items) => {
            Dynamic::from_array(items.iter().map(cbor_to_dynamic).collect())
        }
        CborValue::Map(pairs) => {
            let mut map = RhaiMap::new();
            for (k, v) in pairs {
                if let CborValue::Text(key) = k {
                    map.insert(key.as_str().into(), cbor_to_dynamic(v));
                }
            }
            Dynamic::from_map(map)
        }
        CborValue::Tag(_, inner) => cbor_to_dynamic(inner),
        _ => Dynamic::UNIT,
    }
}

/// Dynamic → CBOR. Errors on types with no CBOR story (closures, custom
/// types) — a script setting one on a servo is a bug worth naming.
pub fn dynamic_to_cbor(value: &Dynamic) -> Result<CborValue, Box<EvalAltResult>> {
    if value.is_unit() {
        return Ok(CborValue::Null);
    }
    if let Some(b) = value.clone().try_cast::<bool>() {
        return Ok(CborValue::Bool(b));
    }
    if let Some(i) = value.clone().try_cast::<i64>() {
        return Ok(CborValue::Integer(i.into()));
    }
    if let Some(f) = value.clone().try_cast::<f64>() {
        return Ok(CborValue::Float(f));
    }
    if let Some(s) = value.clone().try_cast::<String>() {
        return Ok(CborValue::Text(s));
    }
    if let Some(blob) = value.clone().try_cast::<rhai::Blob>() {
        return Ok(CborValue::Bytes(blob));
    }
    if let Some(array) = value.clone().try_cast::<rhai::Array>() {
        let items = array
            .iter()
            .map(dynamic_to_cbor)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(CborValue::Array(items));
    }
    if let Some(map) = value.clone().try_cast::<RhaiMap>() {
        let pairs = map
            .iter()
            .map(|(k, v)| Ok((CborValue::Text(k.to_string()), dynamic_to_cbor(v)?)))
            .collect::<Result<Vec<_>, Box<EvalAltResult>>>()?;
        return Ok(CborValue::Map(pairs));
    }
    Err(format!("no CBOR representation for rhai type '{}'", value.type_name()).into())
}
