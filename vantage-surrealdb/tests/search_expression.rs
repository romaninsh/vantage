use ciborium::Value as CborValue;
use surreal_client::{MockSurrealEngine, SurrealClient};
use vantage_expressions::Expressive;
use vantage_surrealdb::identifier::Identifier;
use vantage_surrealdb::operation::SurrealOperation;
use vantage_surrealdb::select::SurrealSelect;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::vista::factory::SurrealVistaFactory;
use vantage_table::prelude::*;
use vantage_table::table::{Aggregate, RelationQuery};
use vantage_types::EmptyEntity;
use vantage_vista::FilterOp;

fn make_db() -> SurrealDB {
    let client = SurrealClient::new(
        Box::new(MockSurrealEngine::new()),
        Some("test_db".to_string()),
        Some("test_ns".to_string()),
    );
    SurrealDB::new(client)
}

/// How conditions make groups. `AND` binds more tightly than `OR`. Thus
/// alternatives without brackets change the query: `status = 'x' AND a
/// OR b` means `(status = 'x' AND a) OR b`, and each row that matches
/// only `b` ignores the status condition. `or_` and `and_` write the
/// brackets. The renderer does not.
#[test]
fn test_condition_grouping() {
    let status = Identifier::new("status").eq("registered");
    let a = Identifier::new("label").eq("a");
    let b = Identifier::new("label").eq("b");
    let c = Identifier::new("label").eq("c");

    // A chain is one flat group. It has one set of brackets around the
    // full chain, and no brackets around each operand.
    assert_eq!(
        a.or_(b.clone()).or_(c.clone()).expr().preview(),
        "(label = \"a\" OR label = \"b\" OR label = \"c\")"
    );

    // Nest the calls, and the inner group writes its own brackets.
    assert_eq!(
        a.or_(b.clone().or_(c.clone())).expr().preview(),
        "(label = \"a\" OR (label = \"b\" OR label = \"c\"))"
    );

    // The operator changes. The chain to this point becomes one operand.
    assert_eq!(
        a.or_(b.clone()).and_(c.clone()).expr().preview(),
        "((label = \"a\" OR label = \"b\") AND label = \"c\")"
    );

    // Each condition that you add goes into the query with a plain AND.
    // The query has no more brackets, because each condition is
    // complete.
    let select = SurrealSelect::new()
        .from("tag")
        .field("id")
        .with_where(status.clone())
        .with_where(a.or_(b).or_(c));
    assert_eq!(
        select.preview(),
        "SELECT id FROM tag WHERE status = \"registered\" \
         AND (label = \"a\" OR label = \"b\" OR label = \"c\")"
    );
}

/// A search looks in each column, and it obeys the same rule. The
/// branches make one group, and thus the conditions of the table stay
/// in effect.
#[test]
fn test_search_across_columns() {
    let db = make_db();

    let table = Table::<SurrealDB, EmptyEntity>::new("tag", db.clone())
        .with_column(Column::<String>::new("id").with_flag(ColumnFlag::IdField))
        .with_column(Column::<String>::new("label"))
        // The query writes a computed column as `(<expr>) AS <name>`.
        // The alias does not exist where WHERE runs, and a search on the
        // alias matches no rows. The search uses the expression.
        .with_column(Column::<String>::new("owner_email"))
        .with_expression("owner_email", |_| {
            vantage_surrealdb::surreal_expr!("owner.email")
        })
        .with_condition(Identifier::new("status").eq("registered"));

    let query = table
        .select()
        .with_where(db.search_table_condition(&table, "PG9W"))
        .preview();

    assert_eq!(
        query,
        "SELECT id, label, (owner.email) AS owner_email FROM tag \
         WHERE status = \"registered\" AND (\
         string::contains(string::lowercase(<string>(id)), \"pg9w\") OR \
         string::contains(string::lowercase(<string>(label)), \"pg9w\") OR \
         string::contains(string::lowercase(<string>(owner.email)), \"pg9w\"))"
    );

    // A table with no columns to search matches no rows.
    let empty = Table::<SurrealDB, EmptyEntity>::new("empty", db.clone());
    assert_eq!(db.search_table_condition(&empty, "x").preview(), "false");
}

fn line_table(db: SurrealDB) -> Table<SurrealDB, EmptyEntity> {
    Table::new("line", db)
        .with_id_column("id")
        .with_column_of::<bool>("void")
}

/// A condition on an `exists()`, a `count()` and a computed boolean
/// column filters by the column's expression, as a search does.
#[test]
fn condition_on_expression_columns() {
    let db = make_db();
    let invoices = Table::<SurrealDB, EmptyEntity>::new("invoice", db.clone())
        .with_id_column("id")
        .with_column_of::<i64>("net")
        .with_many("lines", "invoice", line_table)
        .with_column_of::<bool>("voided")
        .with_relation_column(
            "voided",
            RelationQuery::new("lines").with_condition(surreal_expr!("void = {}", true)),
            Aggregate::Exists,
        )
        .unwrap()
        .with_column_of::<i64>("n_lines")
        .with_relation_column("n_lines", RelationQuery::new("lines"), Aggregate::Count)
        .unwrap()
        .with_column_of::<bool>("large")
        .with_expression("large", |_| surreal_expr!("net > 100"));
    let vista = SurrealVistaFactory::new(db).from_table(invoices).unwrap();
    let lines = "SELECT VALUE id FROM line WHERE invoice = $parent.id";
    let cases = [
        (
            "voided",
            FilterOp::Eq,
            CborValue::Bool(true),
            format!("((count(({lines} AND void = true)) > 0)) = true"),
        ),
        (
            "n_lines",
            FilterOp::Gt,
            CborValue::Integer(1.into()),
            format!("(count(({lines}))) > 1"),
        ),
        (
            "large",
            FilterOp::Eq,
            CborValue::Bool(true),
            "(net > 100) = true".to_string(),
        ),
    ];
    for (column, op, value, filter) in cases {
        let query = vista
            .clone()
            .with_condition(column, op, value)
            .unwrap()
            .preview()
            .query
            .unwrap();
        let (_, rendered) = query
            .rsplit_once(" FROM invoice WHERE ")
            .unwrap_or_else(|| panic!("{query}"));
        assert_eq!(rendered, filter, "{column}");
    }
}
