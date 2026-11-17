//! A rule script's `with_condition` hands the rule target a `SqliteCondition`.

use vantage_sql::condition::SqliteCondition;
use vantage_sql::sqlite::vista::register_rule_conditions;

use crate::rule_conditions::rule_conditions;

fn render(condition: Box<dyn std::any::Any + Send>) -> String {
    let condition = condition
        .downcast::<SqliteCondition>()
        .expect("a SqliteCondition");
    condition.into_expr().preview()
}

#[test]
fn rule_with_condition_narrows_sqlite() {
    let rendered = rule_conditions(
        register_rule_conditions,
        render,
        r#"table.with_condition(sql::expr("bakery_id = {}", [3]))"#,
    );
    assert_eq!(rendered, ["bakery_id = 3"]);
}
