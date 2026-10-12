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
use vantage_vista::{Capability, FilterOp, ReferenceKind, SortDirection, Vista};

fn aws() -> AwsAccount {
    AwsAccount::new("AKIATEST", "secret", "eu-west-2")
}

/// Relation names and kinds, in declaration order.
fn ref_kinds(vista: &Vista) -> Vec<(String, ReferenceKind)> {
    vista
        .references()
        .values()
        .map(|r| (r.name.clone(), r.kind))
        .collect()
}

#[test]
fn capabilities_advertise_read_only() -> Result<()> {
    let factory = aws().vista_factory();
    let vista = factory.from_table(iam::users_table(aws()))?;

    let caps: Vec<Capability> = vista.capabilities().iter().collect();
    assert_eq!(caps, [Capability::Traverse]);
    assert_eq!(vista.driver_name(), "aws");

    // S3 object listings page by key as well.
    let objects = factory.from_table(s3::objects_table(aws()))?;
    let caps: Vec<Capability> = objects.capabilities().iter().collect();
    assert_eq!(caps, [Capability::Cursor, Capability::Traverse]);
    Ok(())
}

#[test]
fn metadata_round_trip_from_iam_users() -> Result<()> {
    let factory = aws().vista_factory();
    let vista = factory.from_table(iam::users_table(aws()))?;

    assert_eq!(vista.name(), "query/Users:iam/2010-05-08.ListUsers");
    assert_eq!(vista.id_column(), Some("UserName"));

    for name in ["UserName", "Arn", "CreateDate", "PasswordLastUsed"] {
        assert!(vista.column(name).is_some(), "missing column `{name}`");
    }

    // Title columns come from the typed table's `with_title_column_of`.
    let titles = vista.title_columns();
    assert!(titles.contains(&"Path"));
    assert!(titles.contains(&"CreateDate"));
    Ok(())
}

#[test]
fn ref_kinds_match_typed_relations() -> Result<()> {
    let factory = aws().vista_factory();

    let users = factory.from_table(iam::users_table(aws()))?;
    let user_refs = ref_kinds(&users);
    assert_eq!(
        user_refs,
        vec![
            ("groups".into(), ReferenceKind::HasMany),
            ("access_keys".into(), ReferenceKind::HasMany),
            ("attached_policies".into(), ReferenceKind::HasMany),
        ],
    );

    let functions = factory.from_table(lambda::functions_table(aws()))?;
    let function_refs = ref_kinds(&functions);
    assert_eq!(
        function_refs,
        vec![
            ("aliases".into(), ReferenceKind::HasMany),
            ("versions".into(), ReferenceKind::HasMany),
        ],
    );

    let groups = factory.from_table(logs::groups_table(aws()))?;
    let group_refs = ref_kinds(&groups);
    assert_eq!(
        group_refs,
        vec![
            ("events".into(), ReferenceKind::HasMany),
            ("streams".into(), ReferenceKind::HasMany),
        ],
    );

    let clusters = factory.from_table(ecs::clusters_table(aws()))?;
    let cluster_refs = ref_kinds(&clusters);
    assert_eq!(
        cluster_refs,
        vec![
            ("services".into(), ReferenceKind::HasMany),
            ("tasks".into(), ReferenceKind::HasMany),
        ],
    );

    let buckets = factory.from_table(s3::buckets_table(aws()))?;
    let bucket_refs = ref_kinds(&buckets);
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

    // Each refusal names the capability the shell doesn't advertise.
    let capability = |e: &vantage_core::VantageError| e.context.get("capability").cloned();

    let insert_err = vista
        .insert_value(id.clone(), &empty)
        .await
        .expect_err("insert should be unsupported on a read-only driver");
    assert_eq!(insert_err.kind(), ErrorKind::Unsupported);
    assert_eq!(capability(&insert_err).as_deref(), Some("\"insert\""));

    let replace_err = vista
        .replace_value(id.clone(), &empty)
        .await
        .expect_err("replace should be unsupported");
    assert_eq!(replace_err.kind(), ErrorKind::Unsupported);
    assert_eq!(capability(&replace_err).as_deref(), Some("\"update\""));

    let delete_err = vista
        .delete(id.clone())
        .await
        .expect_err("delete should be unsupported");
    assert_eq!(delete_err.kind(), ErrorKind::Unsupported);
    assert_eq!(capability(&delete_err).as_deref(), Some("\"delete\""));
    Ok(())
}

/// None of these issue a request: each is refused before any I/O.
#[tokio::test]
async fn query_primitives_return_unsupported() -> Result<()> {
    let factory = aws().vista_factory();
    let mut vista = factory.from_table(iam::users_table(aws()))?;

    let order_err = vista
        .set_order(Some(("UserName", SortDirection::Ascending)))
        .expect_err("AWS doesn't advertise order");
    assert_eq!(order_err.kind(), ErrorKind::Unsupported);

    let search_err = vista
        .set_search(Some("anything"))
        .expect_err("AWS doesn't advertise search");
    assert_eq!(search_err.kind(), ErrorKind::Unsupported);

    let op_err = vista
        .add_condition("UserName", FilterOp::Ne, "x")
        .expect_err("AWS doesn't advertise filter_ops");
    assert_eq!(op_err.kind(), ErrorKind::Unsupported);

    let window_err = vista
        .fetch_window(0, 10)
        .await
        .expect_err("AWS doesn't advertise window");
    assert_eq!(window_err.kind(), ErrorKind::Unsupported);

    let cursor_err = vista
        .fetch_cursor(None, 10)
        .await
        .expect_err("IAM users don't page by key");
    assert_eq!(cursor_err.kind(), ErrorKind::Unsupported);
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

    let arn = vista.column("Arn").expect("Arn column exists");
    assert!(
        arn.original_type.ends_with("::Arn"),
        "expected Arn column to carry an ::Arn-suffixed type name, got `{}`",
        arn.original_type,
    );

    let created = vista
        .column("CreateDate")
        .expect("CreateDate column exists");
    assert!(
        created.original_type.ends_with("::AwsDateTime"),
        "expected CreateDate column to carry an ::AwsDateTime-suffixed type name, got `{}`",
        created.original_type,
    );

    let user_name = vista.column("UserName").expect("UserName column exists");
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
fn add_condition_on_a_declared_column_is_recorded() -> Result<()> {
    // The behavioural side of pushdown (the condition actually shaping
    // the AWS request body) is verified by the unit tests in
    // `src/vista/source/tests.rs`, which introspect the wrapped table.
    let factory = aws().vista_factory();
    let vista = factory
        .from_table(iam::users_table(aws()))?
        .with_condition("UserName", FilterOp::Eq, CborValue::Text("alice".into()))?;
    assert!(vista.is_narrowed());

    // A declared request parameter narrows like a column; a field the table
    // doesn't declare is an error, not a silent pass.
    vista
        .clone()
        .with_condition("PathPrefix", FilterOp::Eq, "/admin/")?;
    let err = vista
        .clone()
        .with_condition("Nickname", FilterOp::Eq, "al")
        .unwrap_err();
    assert_ne!(err.kind(), ErrorKind::Unsupported);
    Ok(())
}

/// Every relation traversed from a parent row comes back narrowed by its
/// join key, so its target takes the join column. Building and narrowing a
/// target issues no request.
#[test]
fn every_relation_narrows_its_target_from_a_row() {
    let factory = Factory::new(aws());
    let mut failures = Vec::new();
    for name in Factory::known_names() {
        let (vista, _) = factory.for_name(name).unwrap();
        for reference in vista.references().values() {
            let row: Record<CborValue> = reference
                .keys
                .iter()
                .map(|k| (k.from.clone(), CborValue::Text("x".into())))
                .collect();
            match vista.get_ref(&reference.name, Some(&row)) {
                Ok(target) if target.is_narrowed() => {}
                Ok(_) => failures.push(format!("{name}:{} is not narrowed", reference.name)),
                Err(e) => failures.push(format!("{name}:{}: {e:?}", reference.name)),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
