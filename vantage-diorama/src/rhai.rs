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
use vantage_rhai::Vocab;
use vantage_rhai::rhai::{Dynamic, Engine, EvalAltResult, Map as RhaiMap};
// One converter for the whole data layer: a record id renders as `table:id`
// in a servo script exactly as it does in a vista script.
pub use vantage_vista::{cbor_to_dynamic, record_to_map};

use crate::servo::{Servo, ServoStatus};

/// The `Servo` type and its methods as a [`Vocab`]. The host decides how a
/// script *obtains* a servo — an [`Env`](vantage_rhai::Env) variable, a
/// registered lookup fn — this only covers what a script can do once it
/// holds one.
pub struct ServoVocab;

impl Vocab for ServoVocab {
    fn register(&self, engine: &mut Engine) {
        register_servo_onto(engine);
    }
}

/// Teach `engine` the `Servo` type and its methods. Prefer [`ServoVocab`] on
/// a host — this is the registration behind it.
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
        |servo: &mut Arc<Servo>, field: &str, value: Dynamic| -> Result<(), Box<EvalAltResult>> {
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
                map.insert(
                    "message".into(),
                    Dynamic::from(rejection.message().to_string()),
                );
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

/// Dynamic → CBOR. An instant (a host's `now()`) travels as the standard
/// CBOR datetime — tag 0 over RFC 3339 text — which every driver that has a
/// datetime type reads as one; the SurrealDB driver accepts it beside its
/// own compact tag 12. That case is diorama's (it owns `chrono` under the
/// `rhai` feature); everything else is vista's one converter, which errors
/// on types with no CBOR story — a script setting a closure on a servo is a
/// bug worth naming.
pub fn dynamic_to_cbor(value: &Dynamic) -> Result<CborValue, Box<EvalAltResult>> {
    if let Some(dt) = value.clone().try_cast::<chrono::DateTime<chrono::Utc>>() {
        return Ok(CborValue::Tag(
            0,
            Box::new(CborValue::Text(dt.to_rfc3339())),
        ));
    }
    vantage_vista::dynamic_to_cbor(value.clone())
}
