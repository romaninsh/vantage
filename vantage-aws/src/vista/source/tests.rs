use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Condition, FilterOp, TableShell, VistaCapabilities};

use super::AwsTableShell;
use super::cursor::{page_request, pages_by_key};
use crate::AwsAccount;
use crate::condition::AwsCondition;
use crate::impls::table_source::retain_eq_conditions;
use crate::models::{iam, s3};
use crate::vista::factory::metadata_from_table;

mod leading;

fn aws() -> AwsAccount {
    AwsAccount::new("AKIATEST", "secret", "eu-west-2")
}

fn shell_for_iam_users() -> AwsTableShell {
    let table = iam::users_table(aws()).into_entity::<EmptyEntity>();
    let metadata = metadata_from_table(&table);
    AwsTableShell::new(table, VistaCapabilities::new(), metadata)
}

#[test]
fn add_condition_pushes_aws_condition_eq_onto_wrapped_table() {
    // `dispatch` reads `table.conditions()` when assembling the request
    // body; a missing or mistranslated condition is invisible without this
    // introspection.
    let mut shell = shell_for_iam_users();
    shell
        .add_condition(&Condition::new("UserName", FilterOp::Eq, "alice"))
        .expect("add_condition");

    let conditions: Vec<&AwsCondition> = shell.table.conditions().collect();
    assert_eq!(conditions.len(), 1);
    match conditions[0] {
        AwsCondition::Eq { field, value } => {
            assert_eq!(field, "UserName");
            assert_eq!(value, &CborValue::Text("alice".into()));
        }
        other => panic!("expected AwsCondition::Eq, got {other:?}"),
    }
}

#[test]
fn a_refused_condition_leaves_the_table_unchanged() {
    let mut shell = shell_for_iam_users();
    let undeclared = shell.add_condition(&Condition::new("Nickname", FilterOp::Eq, "al"));
    assert!(!undeclared.unwrap_err().is_unsupported());
    let op = shell.add_condition(&Condition::new("UserName", FilterOp::Ne, "alice"));
    assert!(op.unwrap_err().is_unsupported());
    assert_eq!(shell.table.conditions().count(), 0);
}

#[test]
fn declared_request_params_take_a_condition() {
    use crate::models::logs;
    let shells = [
        (
            metadata_shell(logs::groups_table(aws()).into_entity()),
            "logGroupNamePrefix",
        ),
        (shell_for_iam_users(), "PathPrefix"),
        (
            metadata_shell(iam::roles_table(aws()).into_entity()),
            "PathPrefix",
        ),
        (
            metadata_shell(s3::objects_table(aws()).into_entity()),
            "prefix",
        ),
    ];
    for (mut shell, param) in shells {
        shell
            .add_condition(&Condition::new(param, FilterOp::Eq, "/x/"))
            .unwrap_or_else(|e| panic!("{param}: {e}"));
    }
}

fn metadata_shell(table: vantage_table::table::Table<AwsAccount, EmptyEntity>) -> AwsTableShell {
    let metadata = metadata_from_table(&table);
    AwsTableShell::new(table, VistaCapabilities::new(), metadata)
}

#[test]
fn a_path_placeholder_takes_a_condition_undeclared() {
    let table = s3::objects_table(aws()).into_entity::<EmptyEntity>();
    let metadata = metadata_from_table(&table);
    let mut shell = AwsTableShell::new(table, VistaCapabilities::new(), metadata);
    shell
        .add_condition(&Condition::new("Bucket", FilterOp::Eq, "photos"))
        .expect("`{Bucket}` is a path placeholder");
}

#[test]
fn a_page_request_adds_paging_params_after_the_set_conditions() {
    let set = [AwsCondition::eq("Bucket", "photos")];
    let request = page_request(&set, Some(CborValue::Text("k9".into())), 5).unwrap();
    let fields: Vec<&str> = request.iter().map(AwsCondition::field).collect();
    assert_eq!(fields, ["Bucket", "max-keys", "start-after"]);

    let bad_token = page_request(&set, Some(CborValue::Integer(3.into())), 5);
    assert!(bad_token.is_err());
}

#[tokio::test]
async fn a_zero_limit_cursor_is_an_error_before_any_request() {
    let table = s3::objects_table(aws()).into_entity::<EmptyEntity>();
    let metadata = metadata_from_table(&table);
    let caps = VistaCapabilities::new().with(vantage_vista::Capability::Cursor);
    let shell = AwsTableShell::new(table, caps, metadata);
    let err = shell.fetch_cursor(None, 0).await.unwrap_err();
    assert!(!err.is_unsupported(), "{err:?}");
    assert!(err.to_string().contains("limit"), "{err}");
}

/// `Path` is a row field of the user's groups; `UserName` is the request
/// parameter the rows don't carry.
#[test]
fn a_row_missing_a_row_field_fails_and_one_missing_a_request_param_passes() {
    let table = iam::group::groups_for_user_table(aws());
    let required = table.row_fields();
    let rows = || {
        let with_path: Record<CborValue> = [("Path".to_string(), CborValue::from("/ops/"))]
            .into_iter()
            .collect();
        IndexMap::from([
            ("a".to_string(), with_path),
            ("b".to_string(), Record::new()),
        ])
    };

    let mut by_field = rows();
    retain_eq_conditions(
        &mut by_field,
        &[AwsCondition::eq("Path", "/ops/")],
        &required,
    );
    assert_eq!(by_field.keys().collect::<Vec<_>>(), ["a"]);

    let mut by_param = rows();
    retain_eq_conditions(
        &mut by_param,
        &[AwsCondition::eq("UserName", "alice")],
        &required,
    );
    assert_eq!(by_param.keys().collect::<Vec<_>>(), ["a", "b"]);
}

/// AWS has no offset, so it doesn't advertise `Window`: the Vista refuses
/// any offset past the leading window before a request is sent.
#[tokio::test]
async fn an_offset_window_is_refused_before_any_request() {
    let vista = aws()
        .vista_factory()
        .from_table(iam::users_table(aws()))
        .unwrap();
    assert!(!vista.capabilities().has(vantage_vista::Capability::Window));
    let err = vista.fetch_window(1, 5).await.unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
}

/// A request parameter is neither shown nor carried by rows.
#[test]
fn request_params_are_hidden_request_param_columns() {
    use vantage_vista::ColumnFlag;
    let vista = aws()
        .vista_factory()
        .from_table(iam::group::groups_for_user_table(aws()))
        .unwrap();
    let column = vista.column("UserName").expect("UserName");
    assert!(column.has_flag(ColumnFlag::Hidden));
    assert!(column.has_flag(ColumnFlag::RequestParam));
}

#[test]
fn only_s3_object_listings_page_by_key() {
    let objects = s3::objects_table(aws());
    let buckets = s3::buckets_table(aws());
    assert!(pages_by_key(objects.table_name()));
    assert!(!pages_by_key(buckets.table_name()));
    assert!(!pages_by_key(iam::users_table(aws()).table_name()));
}
