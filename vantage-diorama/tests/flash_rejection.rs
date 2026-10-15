//! `FlashRejection`: a structured refusal riding a `VantageError` back to the
//! form that sent the flash.

use vantage_core::Context as _;
use vantage_diorama::FlashRejection;

fn rejection() -> FlashRejection {
    FlashRejection::new("validation failed")
        .with_field("price", "must be positive")
        .with_field("name", "required")
}

#[test]
fn a_rejection_names_its_fields_in_order() {
    let rejection = rejection();
    assert_eq!(rejection.message(), "validation failed");
    assert_eq!(
        rejection.field_errors(),
        [
            ("price".to_string(), "must be positive".to_string()),
            ("name".to_string(), "required".to_string()),
        ]
    );
    assert_eq!(rejection.error_for("name"), Some("required"));
    assert_eq!(rejection.error_for("stock"), None);
}

#[test]
fn a_rejection_round_trips_through_an_error() {
    let err = rejection().into_error();
    assert_eq!(FlashRejection::from_error(&err), Some(rejection()));
    assert!(
        err.to_string().contains("validation failed"),
        "the error reads as the rejection's message: {err}"
    );
}

#[test]
fn a_rejection_is_found_under_extra_context() {
    let wrapped = Err::<(), _>(rejection().into_error())
        .context(vantage_core::error!("route rejected the flash"))
        .unwrap_err();
    let found = FlashRejection::from_error(&wrapped).expect("found through the chain");
    assert_eq!(found.error_for("price"), Some("must be positive"));
}

#[test]
fn an_unstructured_failure_becomes_a_message_only_rejection() {
    let err = vantage_core::error!("disk on fire");
    assert_eq!(FlashRejection::from_error(&err), None);
    let fallback = FlashRejection::from_error_or_message(&err);
    assert!(fallback.message().contains("disk on fire"), "{fallback}");
    assert!(fallback.field_errors().is_empty());
}

#[test]
fn a_structured_failure_is_kept_by_the_fallback() {
    let err = rejection().into_error();
    assert_eq!(FlashRejection::from_error_or_message(&err), rejection());
}

#[test]
fn the_display_carries_the_message_and_every_named_field() {
    let shown = rejection().to_string();
    for part in [
        "validation failed",
        "price",
        "must be positive",
        "name",
        "required",
    ] {
        assert!(shown.contains(part), "{part} missing from {shown}");
    }
    assert_eq!(FlashRejection::new("nope").to_string(), "nope");
}
