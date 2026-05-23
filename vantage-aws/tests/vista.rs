//! Offline contract tests for the AWS Vista driver.
//!
//! These tests don't talk to AWS — they only assert metadata, capability
//! advertisement, and error-kind boundaries that hold whether or not
//! credentials are configured. Live-fetch tests would belong alongside
//! `dynamodb_live.rs`; this file is the part that can run on any
//! developer machine and in CI without secrets.

use ciborium::Value as CborValue;
use vantage_aws::AwsAccount;
use vantage_aws::models::{Factory, FactoryMode, ecs, iam, lambda, logs, s3};
use vantage_core::{ErrorKind, Result};
use vantage_dataset::prelude::WritableValueSet;
use vantage_types::Record;
use vantage_vista::{ReferenceKind, SortDirection};

fn aws() -> AwsAccount {
    AwsAccount::new("AKIATEST", "secret", "eu-west-2")
}

#[test]
fn capabilities_advertise_read_only() -> Result<()> {
    let factory = aws().vista_factory();
    let vista = factory.from_table(iam::users_table(aws()))?;

    let caps = vista.capabilities();
    assert!(
        caps.can_count,
        "AWS list endpoints can be counted (by exhausting them)"
    );
    assert!(!caps.can_insert);
    assert!(!caps.can_update);
    assert!(!caps.can_delete);
    assert!(!caps.can_subscribe);
    assert!(!caps.can_order);
    assert!(!caps.can_search);
    assert!(!caps.can_set_page_size);
    assert!(!caps.can_fetch_page);
    assert!(!caps.can_fetch_next);

    assert_eq!(vista.driver(), "aws");
    Ok(())
}

#[test]
fn metadata_round_trip_from_iam_users() -> Result<()> {
    let factory = aws().vista_factory();
    let vista = factory.from_table(iam::users_table(aws()))?;

    assert_eq!(vista.name(), "query/Users:iam/2010-05-08.ListUsers");
    assert_eq!(vista.get_id_column(), Some("UserName"));

    let names = vista.get_column_names();
    assert!(names.contains(&"UserName"));
    assert!(names.contains(&"Arn"));
    assert!(names.contains(&"CreateDate"));
    assert!(names.contains(&"PasswordLastUsed"));

    // Title columns come from the typed table's `with_title_column_of`.
    let titles = vista.get_title_columns();
    assert!(titles.contains(&"Path"));
    assert!(titles.contains(&"CreateDate"));
    Ok(())
}

#[test]
fn ref_kinds_match_typed_relations() -> Result<()> {
    let factory = aws().vista_factory();

    let users = factory.from_table(iam::users_table(aws()))?;
    let user_refs = users.list_references();
    assert_eq!(
        user_refs,
        vec![
            ("groups".into(), ReferenceKind::HasMany),
            ("access_keys".into(), ReferenceKind::HasMany),
            ("attached_policies".into(), ReferenceKind::HasMany),
        ],
    );

    let functions = factory.from_table(lambda::functions_table(aws()))?;
    let function_refs = functions.list_references();
    assert_eq!(
        function_refs,
        vec![
            ("aliases".into(), ReferenceKind::HasMany),
            ("versions".into(), ReferenceKind::HasMany),
        ],
    );

    let groups = factory.from_table(logs::groups_table(aws()))?;
    let group_refs = groups.list_references();
    assert_eq!(
        group_refs,
        vec![
            ("events".into(), ReferenceKind::HasMany),
            ("streams".into(), ReferenceKind::HasMany),
        ],
    );

    let clusters = factory.from_table(ecs::clusters_table(aws()))?;
    let cluster_refs = clusters.list_references();
    assert_eq!(
        cluster_refs,
        vec![
            ("services".into(), ReferenceKind::HasMany),
            ("tasks".into(), ReferenceKind::HasMany),
        ],
    );

    let buckets = factory.from_table(s3::buckets_table(aws()))?;
    let bucket_refs = buckets.list_references();
    assert_eq!(
        bucket_refs,
        vec![("objects".into(), ReferenceKind::HasMany)],
    );
    Ok(())
}

#[tokio::test]
async fn writes_return_unsupported() -> Result<()> {
    let factory = aws().vista_factory();
    let vista = factory.from_table(iam::users_table(aws()))?;

    let empty = Record::new();
    let id = "anyone".to_string();

    let insert_err = vista
        .insert_value(&id, &empty)
        .await
        .expect_err("insert should be unsupported on a read-only driver");
    assert_eq!(insert_err.kind(), ErrorKind::Unsupported);
    assert!(
        insert_err.to_string().contains("can_insert"),
        "expected capability name in message, got: {insert_err}",
    );

    let replace_err = vista
        .replace_value(&id, &empty)
        .await
        .expect_err("replace should be unsupported");
    assert_eq!(replace_err.kind(), ErrorKind::Unsupported);
    assert!(replace_err.to_string().contains("can_update"));

    let delete_err = vista
        .delete(&id)
        .await
        .expect_err("delete should be unsupported");
    assert_eq!(delete_err.kind(), ErrorKind::Unsupported);
    assert!(delete_err.to_string().contains("can_delete"));
    Ok(())
}

#[tokio::test]
async fn query_primitives_return_unsupported() -> Result<()> {
    let factory = aws().vista_factory();
    let mut vista = factory.from_table(iam::users_table(aws()))?;

    let order_err = vista
        .add_order("UserName", SortDirection::Ascending)
        .expect_err("AWS doesn't advertise can_order");
    assert_eq!(order_err.kind(), ErrorKind::Unsupported);

    let search_err = vista
        .add_search("anything")
        .expect_err("AWS doesn't advertise can_search");
    assert_eq!(search_err.kind(), ErrorKind::Unsupported);

    let size_err = vista
        .set_page_size(10)
        .expect_err("AWS doesn't advertise can_set_page_size");
    assert_eq!(size_err.kind(), ErrorKind::Unsupported);

    let page_err = vista
        .fetch_page(1)
        .await
        .expect_err("AWS doesn't advertise can_fetch_page");
    assert_eq!(page_err.kind(), ErrorKind::Unsupported);

    let next_err = vista
        .fetch_next(None)
        .await
        .expect_err("AWS doesn't advertise can_fetch_next");
    assert_eq!(next_err.kind(), ErrorKind::Unsupported);
    Ok(())
}

/// The CLI renderer keys off `Column::original_type` via
/// `AnyAwsType::from_cbor_typed`, which does suffix matching against
/// `"::Arn"` / `"::AwsDateTime"`. A type rename or relocation that
/// breaks that suffix would silently degrade column rendering to plain
/// `Text`. This test pins the suffixes so a rename trips CI instead.
#[test]
fn column_original_type_carries_renderer_keys() -> Result<()> {
    let factory = aws().vista_factory();
    let vista = factory.from_table(iam::users_table(aws()))?;

    let arn = vista.get_column("Arn").expect("Arn column exists");
    assert!(
        arn.original_type.ends_with("::Arn"),
        "expected Arn column to carry an ::Arn-suffixed type name, got `{}`",
        arn.original_type,
    );

    let created = vista
        .get_column("CreateDate")
        .expect("CreateDate column exists");
    assert!(
        created.original_type.ends_with("::AwsDateTime"),
        "expected CreateDate column to carry an ::AwsDateTime-suffixed type name, got `{}`",
        created.original_type,
    );

    let user_name = vista
        .get_column("UserName")
        .expect("UserName column exists");
    assert_eq!(user_name.original_type, "alloc::string::String");
    Ok(())
}

#[test]
fn factory_for_name_covers_every_known_name() {
    let factory = Factory::new(aws());

    for name in Factory::known_names() {
        let (_vista, mode) = factory
            .for_name(name)
            .unwrap_or_else(|| panic!("for_name missing arm for `{name}`"));

        // Every known name is either singular (no trailing 's') or
        // plural; the dispatcher in `Factory::vista_for_name` mirrors
        // that convention, so we can derive the expected mode from the
        // string alone.
        let expected = if name.ends_with('s') {
            FactoryMode::List
        } else {
            FactoryMode::Single
        };
        assert_eq!(
            mode, expected,
            "wrong FactoryMode for `{name}`: got {mode:?}, expected {expected:?}",
        );
    }
}

#[test]
fn add_condition_eq_does_not_error() -> Result<()> {
    // The behavioural side of pushdown (the condition actually shaping
    // the AWS request body) is verified by the unit test inside
    // `src/vista/source.rs` where we can introspect the wrapped table.
    // This integration test just confirms the Vista-facing call shape.
    let factory = aws().vista_factory();
    let mut vista = factory.from_table(iam::users_table(aws()))?;
    vista.add_condition_eq("PathPrefix", CborValue::Text("/admin/".into()))?;
    Ok(())
}
