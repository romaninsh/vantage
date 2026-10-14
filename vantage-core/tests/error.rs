use std::error::Error;
use std::io;
use std::process::{ExitCode, Termination};

use vantage_core::{Context, ErrorKind, VantageError, error, vantage_error};

#[test]
fn no_data_has_a_plain_message() {
    let err = VantageError::no_data();
    assert_eq!(err.to_string(), "No data available");
    assert_eq!(err.kind(), ErrorKind::Generic);
    assert!(err.location().is_none());
}

#[test]
#[allow(deprecated)]
fn no_capability_names_method_and_type() {
    let err = VantageError::no_capability("insert", "ReadOnlyDataSet");
    assert_eq!(
        err.to_string(),
        "Capability insert is not implemented in generic ReadOnlyDataSet"
    );
}

#[test]
fn other_has_message_and_no_location() {
    let err = VantageError::other("boom");
    assert_eq!(err.message(), "boom");
    assert_eq!(err.to_string(), "boom");
    assert!(err.location().is_none());
    assert_eq!(err.kind(), ErrorKind::Generic);
}

#[test]
fn new_keeps_the_given_location() {
    let err = VantageError::new("bad", "a.rs:1:2".to_string());
    assert_eq!(err.message(), "bad");
    assert_eq!(err.location(), Some("a.rs:1:2"));
}

#[test]
fn error_macro_captures_location_and_context() {
    let err = error!("write failed", table = "books", n = 3);
    assert_eq!(err.message(), "write failed");
    assert!(err.location().is_some_and(|l| l.contains("error.rs")));
    assert_eq!(err.context.len(), 2);
    assert!(err.context.contains_key("table"));
    assert!(err.context.contains_key("n"));
}

#[test]
fn display_is_one_line_with_context_in_order() {
    let err = error!("write failed", table = "books", n = 3);
    let shown = err.to_string();
    assert!(shown.starts_with("write failed (table: "));
    let t = shown.find("table:").unwrap();
    let n = shown.find("n: ").unwrap();
    assert!(t < n);
    assert!(shown.ends_with(')'));
    assert!(!shown.contains('\n'));
}

#[test]
fn display_without_context_is_just_the_message() {
    assert_eq!(error!("plain").to_string(), "plain");
}

#[test]
fn vantage_error_macro_formats_arguments() {
    assert_eq!(
        vantage_error!("Test error: {}", 42).to_string(),
        "Test error: 42"
    );
    assert_eq!(vantage_error!("literal").to_string(), "literal");
    let owned = String::from("owned");
    assert_eq!(vantage_error!(owned).to_string(), "owned");
}

#[test]
fn string_and_str_convert_to_errors() {
    let a: VantageError = "from str".into();
    let b: VantageError = String::from("from string").into();
    assert_eq!(a.message(), "from str");
    assert_eq!(b.message(), "from string");
    assert_eq!(a.kind(), ErrorKind::Generic);
}

#[test]
fn io_error_converts_with_source() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "File not found");
    let err = VantageError::from(io_err);
    assert_eq!(err.message(), "IO error");
    assert_eq!(err.to_string(), "IO error: File not found");
    assert_eq!(err.source().unwrap().to_string(), "File not found");
}

#[test]
fn kind_defaults_to_generic() {
    assert_eq!(ErrorKind::default(), ErrorKind::Generic);
    let err = error!("x");
    assert_eq!(err.kind(), ErrorKind::Generic);
    assert!(!err.is_unsupported());
    assert!(!err.is_unimplemented());
    assert!(!err.is_incorrect_usage());
    assert!(!err.is_not_found());
    assert!(!err.is_conflict());
}

#[test]
fn each_mark_sets_exactly_its_kind() {
    let e = error!("x").mark_unsupported();
    assert_eq!(e.kind(), ErrorKind::Unsupported);
    assert!(e.is_unsupported());
    assert!(!e.is_unimplemented() && !e.is_incorrect_usage() && !e.is_not_found());
    assert!(!e.is_conflict());

    let e = error!("x").mark_unimplemented();
    assert_eq!(e.kind(), ErrorKind::Unimplemented);
    assert!(e.is_unimplemented());
    assert!(!e.is_unsupported());

    let e = error!("x").mark_incorrect_usage();
    assert_eq!(e.kind(), ErrorKind::IncorrectUsage);
    assert!(e.is_incorrect_usage());
    assert!(!e.is_unsupported());

    let e = error!("x").mark_not_found();
    assert_eq!(e.kind(), ErrorKind::NotFound);
    assert!(e.is_not_found());
    assert!(!e.is_conflict());

    let e = error!("row belongs to another set", id = "o1").mark_conflict();
    assert_eq!(e.kind(), ErrorKind::Conflict);
    assert!(e.is_conflict());
    assert!(!e.is_not_found());
}

#[test]
fn marking_again_replaces_the_kind() {
    let e = error!("x").mark_unsupported().mark_not_found();
    assert_eq!(e.kind(), ErrorKind::NotFound);
}

#[test]
fn traced_returns_the_same_error() {
    let e = error!("x", id = 1).mark_unsupported().traced();
    assert_eq!(e.kind(), ErrorKind::Unsupported);
    assert_eq!(e.message(), "x");
    assert_eq!(e.context.len(), 1);

    let e = error!("y").mark_not_found().traced_debug();
    assert_eq!(e.kind(), ErrorKind::NotFound);
    assert_eq!(e.message(), "y");
}

#[test]
fn traced_without_context_or_location_is_fine() {
    let e = VantageError::other("z").traced().traced_debug();
    assert_eq!(e.message(), "z");
}

#[test]
fn context_wraps_a_foreign_error() {
    fn failing() -> io::Result<String> {
        Err(io::Error::new(io::ErrorKind::NotFound, "File not found"))
    }

    let err = failing().context("Failed to read file").unwrap_err();
    assert_eq!(err.message(), "Failed to read file");
    assert_eq!(err.to_string(), "Failed to read file: File not found");
    assert_eq!(err.source().unwrap().to_string(), "File not found");
}

#[test]
fn context_leaves_ok_untouched() {
    let ok: Result<u8, io::Error> = Ok(5);
    assert_eq!(ok.context("never used").unwrap(), 5);
}

#[test]
fn context_wraps_a_vantage_error() {
    let inner: Result<(), VantageError> = Err(error!("inner").mark_not_found());
    let err = inner.context("outer").unwrap_err();
    assert_eq!(err.message(), "outer");
    assert_eq!(err.to_string(), "outer: inner");
    let source = err.source().unwrap();
    let source = source.downcast_ref::<VantageError>().unwrap();
    assert_eq!(source.message(), "inner");
    assert!(source.is_not_found());
}

#[test]
fn with_context_builds_the_error_lazily() {
    let ok: Result<u8, io::Error> = Ok(1);
    let value = ok
        .with_context(|| -> VantageError { panic!("must not be called on Ok") })
        .unwrap();
    assert_eq!(value, 1);

    let bad: Result<(), io::Error> = Err(io::Error::other("disk"));
    let err = bad.with_context(|| error!("saving")).unwrap_err();
    assert_eq!(err.to_string(), "saving: disk");

    let bad: Result<(), VantageError> = Err(error!("inner"));
    let err = bad.with_context(|| error!("outer")).unwrap_err();
    assert_eq!(err.to_string(), "outer: inner");
    assert!(err.source().unwrap().is::<VantageError>());
}

#[test]
fn display_includes_context_then_source() {
    let bad: Result<(), io::Error> = Err(io::Error::other("disk"));
    let err = bad.with_context(|| error!("saving", id = 1)).unwrap_err();
    let shown = err.to_string();
    assert!(shown.starts_with("saving (id: "));
    assert!(shown.ends_with("): disk"));
}

#[test]
fn report_returns_failure_for_plain_and_nested_errors() {
    let plain = VantageError::other("plain");
    assert_eq!(
        format!("{:?}", plain.report()),
        format!("{:?}", ExitCode::FAILURE)
    );

    let io_err: Result<(), io::Error> = Err(io::Error::other("disk"));
    let nested: Result<(), VantageError> = io_err.with_context(|| error!("middle", id = 2));
    let outer: Result<(), VantageError> = nested.context(error!("outer", table = "books"));
    let report = outer.unwrap_err().report();
    assert_eq!(format!("{report:?}"), format!("{:?}", ExitCode::FAILURE));
}

#[test]
fn report_handles_error_with_only_location() {
    let err = VantageError::new("only location", "x.rs:1:1".to_string());
    let _ = err.report();
}
