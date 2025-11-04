use vantage_expressions::{expr, protocol::selectable::Selectable};
use vantage_sql::Select;

fn main() {
    // Create a new Select instance
    let mut select = Select::new();

    // Use Selectable trait methods
    select.set_source(expr!("users"), None);
    select.add_field("department".to_string());
    select.add_expression(expr!("COUNT(*)"), Some("total".to_string()));
    select.add_where_condition(expr!("age > 18"));
    select.add_order_by(expr!("total"), false);
    select.add_group_by(expr!("department"));
    select.add_where_condition(expr!("COUNT(*) > 5"));
    select.set_limit(Some(10), Some(5));
    select.set_distinct(true);

    println!("SQL Select with Selectable trait:");
    let expr: vantage_expressions::Expression = select.clone().into();
    println!("{}", expr.preview());
    println!();

    // Test trait query methods
    println!("Trait query methods:");
    println!("Has fields: {}", select.has_fields());
    println!("Has where conditions: {}", select.has_where_conditions());
    println!("Has order by: {}", select.has_order_by());
    println!("Has group by: {}", select.has_group_by());
    println!("Is distinct: {}", select.is_distinct());
    println!("Limit: {:?}", select.get_limit());
    println!("Skip: {:?}", select.get_skip());
    println!();

    // Test clear methods
    select.clear_fields();
    select.clear_where_conditions();
    select.clear_order_by();
    select.clear_group_by();

    println!("After clearing:");
    println!("Has fields: {}", select.has_fields());
    println!("Has where conditions: {}", select.has_where_conditions());
    println!("Has order by: {}", select.has_order_by());
    println!("Has group by: {}", select.has_group_by());
    println!();

    // Basic select example
    let mut basic_select = Select::new();
    basic_select.set_source(expr!("products"), None);
    basic_select.add_field("name".to_string());
    basic_select.add_field("price".to_string());
    basic_select.add_where_condition(expr!("price > 100"));
    basic_select.set_limit(Some(5), None);

    println!("Basic SQL select:");
    let expr: vantage_expressions::Expression = basic_select.into();
    println!("{}", expr.preview());
}
