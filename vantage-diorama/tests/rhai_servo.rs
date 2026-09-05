//! Servo driven from Rhai — the `rhai` feature's contract.
//!
//! Scripts evaluate under `spawn_blocking` (runtime context, no async
//! frame), exactly the posture `save()` documents: it `block_on`s the
//! servo's flash from inside the synchronous script.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::rhai::register_servo_onto;
use vantage_diorama::{Dio, IdStrategy, Lens, Servo};
use vantage_rhai::rhai::{Dynamic, Engine, Scope};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn tag_vista(shell: &MockShell) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("status", "String"))
        .with_id_column("id");
    Vista::new("tags", Box::new(shell.clone().with_metadata(metadata)))
}

async fn dio_over(shell: &MockShell) -> Result<Dio> {
    let lens = Arc::new(Lens::new().cache_in_memory().build().expect("build lens"));
    lens.make_dio(tag_vista(shell)).await
}

/// Evaluate `script` with `servo` in scope, on a blocking thread.
async fn eval(servo: Arc<Servo>, script: &'static str) -> std::result::Result<Dynamic, String> {
    tokio::task::spawn_blocking(move || {
        let mut engine = Engine::new();
        register_servo_onto(&mut engine);
        let mut scope = Scope::new();
        scope.push("servo", servo);
        engine
            .eval_with_scope::<Dynamic>(&mut scope, script)
            .map_err(|e| e.to_string())
    })
    .await
    .expect("eval task")
}

#[tokio::test(flavor = "multi_thread")]
async fn set_save_settles_a_new_record() -> Result<()> {
    let shell = MockShell::new();
    let dio = dio_over(&shell).await?;
    let servo = Arc::new(dio.servo_new(IdStrategy::FromRecord));

    let id = eval(
        servo.clone(),
        r#"
            servo.set("id", "tag:AB12-CD34");
            servo.set("status", "unregistered");
            if !servo.is_dirty() { throw "draft should be dirty"; }
            if !servo.dirty("status") { throw "field should be dirty"; }
            servo.save()
        "#,
    )
    .await
    .expect("script runs");

    assert_eq!(id.into_string().unwrap(), "tag:AB12-CD34");
    assert_eq!(servo.id().as_deref(), Some("tag:AB12-CD34"));
    assert_eq!(
        shell.get_record("tag:AB12-CD34").unwrap().get("status"),
        Some(&text("unregistered")),
        "the record reached the master under the commanded id"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn get_error_status_read_the_draft() -> Result<()> {
    let mut seeded = Record::new();
    seeded.insert("id".to_string(), text("t1"));
    seeded.insert("status".to_string(), text("registered"));
    let shell = MockShell::new().with_record("t1", seeded.clone());
    let dio = dio_over(&shell).await?;
    dio.cache().insert_value("t1", &seeded).await?;
    let servo = Arc::new(dio.servo("t1").await?);

    let result = eval(
        servo,
        r#"
            servo.set("status", "lost");
            let e = servo.error();
            [servo.get("status"), e.status, servo.status(), servo.baseline().status]
        "#,
    )
    .await
    .expect("script runs");

    let values = result.into_array().unwrap();
    assert_eq!(values[0].clone().into_string().unwrap(), "lost");
    assert_eq!(values[1].clone().into_string().unwrap(), "lost");
    assert_eq!(values[2].clone().into_string().unwrap(), "tracking");
    assert_eq!(
        values[3].clone().into_string().unwrap(),
        "registered",
        "baseline still reads the measurement"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_save_is_a_script_error_and_the_draft_survives() -> Result<()> {
    let shell = MockShell::new();
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_flash(|_dio, _flash| async move {
                Err(vantage_core::error!("route rejected the flash"))
            })
            .build()
            .expect("build lens"),
    );
    let dio = lens.make_dio(tag_vista(&shell)).await?;
    let servo = Arc::new(dio.servo_new(IdStrategy::FromRecord));

    let err = eval(
        servo.clone(),
        r#"
            servo.set("id", "tag:XX99-YY88");
            servo.save()
        "#,
    )
    .await
    .expect_err("the rejection surfaces as a script error");
    assert!(err.contains("save failed"), "{err}");

    let status = eval(
        servo.clone(),
        r#"[servo.status(), servo.rejection().message]"#,
    )
    .await
    .expect("status script runs");
    let values = status.into_array().unwrap();
    assert_eq!(values[0].clone().into_string().unwrap(), "failed");
    assert!(
        values[1]
            .clone()
            .into_string()
            .unwrap()
            .contains("rejected"),
        "the rejection carries the route's message"
    );
    assert_eq!(
        servo.get("id"),
        Some(text("tag:XX99-YY88")),
        "the draft survives the rejection"
    );
    Ok(())
}

/// A chrono instant handed to a script (a host's `now()`) writes as the
/// standard CBOR datetime — tag 0 over RFC 3339 — not as plain text.
#[tokio::test(flavor = "multi_thread")]
async fn an_instant_writes_as_a_cbor_datetime() -> Result<()> {
    let shell = MockShell::new();
    let dio = dio_over(&shell).await?;
    let servo = Arc::new(dio.servo_new(IdStrategy::FromRecord));
    let stamp = chrono::Utc::now();

    let servo_for_script = servo.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let mut engine = Engine::new();
        register_servo_onto(&mut engine);
        let mut scope = Scope::new();
        scope.push("servo", servo_for_script);
        scope.push("stamp", stamp);
        engine
            .eval_with_scope::<Dynamic>(&mut scope, r#"servo.set("created", stamp)"#)
            .map_err(|e| e.to_string())
    })
    .await
    .expect("eval task")
    .expect("script runs");

    assert_eq!(
        servo.get("created"),
        Some(CborValue::Tag(0, Box::new(text(&stamp.to_rfc3339()))))
    );
    Ok(())
}
