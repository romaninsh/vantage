//! Setpoints and the error signal: commanding a field, reverting it, and
//! clean fields tracking upstream while another is locked.

use vantage_core::Result;

use crate::support::chunk::wait_for_gen;
use crate::support::text;
use crate::{dio_over, int, product_shell, upstream};

// ---- setpoints, error signal, revert ---------------------------------------

#[tokio::test]
async fn set_commands_a_setpoint_and_raises_the_error_signal() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;

    assert!(!servo.is_dirty(), "freshly opened servo has zero error");

    servo.set("name", text("Tea"));
    assert!(servo.dirty("name"));
    assert!(!servo.dirty("price"), "untouched field stays in tracking");
    let error = servo.error();
    assert_eq!(error.get("name"), Some(&text("Tea")));
    assert_eq!(error.len(), 1, "error signal carries only commanded fields");
    Ok(())
}

#[tokio::test]
async fn set_bumps_generation_for_subscribers() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;
    let mut gen_rx = servo.subscribe();

    let g0 = u64::from(*gen_rx.borrow_and_update());
    servo.set("name", text("Tea"));
    wait_for_gen(&mut gen_rx, g0).await;
    Ok(())
}

#[tokio::test]
async fn revert_releases_the_setpoint_back_to_tracking() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;

    servo.set("name", text("Tea"));
    servo.revert("name");

    assert!(!servo.is_dirty());
    assert_eq!(
        servo.get("name"),
        Some(text("Coffee")),
        "value back to baseline"
    );
    Ok(())
}

#[tokio::test]
async fn setting_the_baseline_value_is_zero_error() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;

    // Commanding the measured value: error is zero by definition.
    servo.set("name", text("Coffee"));
    assert!(!servo.is_dirty());
    Ok(())
}

// ---- continuous tracking while editing -------------------------------------

#[tokio::test]
async fn clean_fields_track_upstream_while_a_field_is_locked() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;
    let mut gen_rx = servo.subscribe();

    servo.set("name", text("Tea")); // lock one field
    let g = u64::from(*gen_rx.borrow_and_update());

    // Upstream reports a new state: stock moved, name unchanged upstream.
    upstream(&dio, "Coffee", 99).await?;
    wait_for_gen(&mut gen_rx, g).await;

    assert_eq!(
        servo.get("stock"),
        Some(int(99)),
        "tracking field followed upstream"
    );
    assert!(!servo.dirty("stock"), "...and stayed clean");
    assert_eq!(
        servo.get("name"),
        Some(text("Tea")),
        "locked field held its setpoint"
    );
    assert!(servo.dirty("name"));
    Ok(())
}

#[tokio::test]
async fn upstream_convergence_zeroes_the_error() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;
    let mut gen_rx = servo.subscribe();

    servo.set("name", text("Tea"));
    let g = u64::from(*gen_rx.borrow_and_update());

    // Upstream independently arrives at the commanded value.
    upstream(&dio, "Tea", 10).await?;
    wait_for_gen(&mut gen_rx, g).await;

    assert!(
        !servo.is_dirty(),
        "error hit zero from the measurement side"
    );
    assert_eq!(servo.get("name"), Some(text("Tea")));
    Ok(())
}
