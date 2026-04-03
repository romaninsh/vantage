#[cfg(test)]
mod tests {
    use crate::statements::update::SurrealUpdate;
    use crate::thing::Thing;
    use crate::types::AnySurrealType;
    use vantage_expressions::Expressive;

    #[test]
    fn test_update_set_basic() {
        let update = SurrealUpdate::new(Thing::new("users", "john"))
            .with_field("name", "John".to_string())
            .with_field("age", 30i64);

        let rendered = update.preview();
        assert!(rendered.starts_with("UPDATE users:john SET"));
        assert!(rendered.contains("name = \"John\""));
        assert!(rendered.contains("age = 30"));
    }

    #[test]
    fn test_update_set_empty() {
        let update = SurrealUpdate::new(Thing::new("users", "john"));
        assert_eq!(update.preview(), "UPDATE users:john");
    }

    #[test]
    fn test_update_content() {
        let update = SurrealUpdate::new(Thing::new("users", "john"))
            .content()
            .with_field("name", "Replaced".to_string())
            .with_field("score", 99i64);

        let rendered = update.preview();
        assert!(rendered.starts_with("UPDATE users:john CONTENT"));
    }

    #[test]
    fn test_update_merge() {
        let update = SurrealUpdate::new(Thing::new("users", "john"))
            .merge()
            .with_field("score", 75i64);

        let rendered = update.preview();
        assert!(rendered.starts_with("UPDATE users:john MERGE"));
    }

    #[test]
    fn test_with_any_field() {
        let val = AnySurrealType::new(42i64);
        let update = SurrealUpdate::new(Thing::new("data", "x")).with_any_field("count", val);
        let rendered = update.preview();
        assert!(rendered.contains("count = 42"));
    }

    #[test]
    fn test_with_record() {
        let mut record = vantage_types::Record::new();
        record.insert("a".to_string(), AnySurrealType::new(1i64));
        record.insert("b".to_string(), AnySurrealType::new("hi".to_string()));

        let update = SurrealUpdate::new(Thing::new("t", "1")).with_record(&record);
        let rendered = update.preview();
        assert!(rendered.contains("a = 1"));
        assert!(rendered.contains("b = \"hi\""));
    }

    #[test]
    fn test_update_identifier_escaping() {
        let update = SurrealUpdate::new(crate::surreal_expr!("⟨SELECT⟩:test"))
            .with_field("FROM", "value".to_string());

        let rendered = update.preview();
        assert!(rendered.contains("⟨FROM⟩ = \"value\""));
    }

    #[test]
    fn test_update_produces_parameterized_expression() {
        let update = SurrealUpdate::new(Thing::new("t", "1"))
            .with_field("x", 10i64)
            .with_field("y", 20i64);

        let expr = update.expr();
        assert!(expr.template.contains("{}"));
        assert_eq!(expr.parameters.len(), 3); // target + 2 fields
    }

    #[test]
    fn test_update_with_thing_field() {
        let update = SurrealUpdate::new(Thing::new("order", "o1"))
            .with_field("customer", Thing::new("user", "alice"));

        let rendered = update.preview();
        assert!(rendered.contains("UPDATE order:o1 SET"));
        assert!(rendered.contains("customer ="));
    }

    #[test]
    fn test_mode_switching() {
        let update = SurrealUpdate::new(Thing::new("t", "1"))
            .content()
            .with_field("a", 1i64);
        assert!(update.preview().contains("CONTENT"));

        let update = update.merge();
        assert!(update.preview().contains("MERGE"));

        let update = update.set();
        assert!(update.preview().contains("SET"));
    }

    #[test]
    fn test_with_condition() {
        let update = SurrealUpdate::table("users")
            .with_field("active", false)
            .with_condition(crate::surreal_expr!("last_login < {}", "2020-01-01"));

        let p = update.preview();
        assert!(p.contains("UPDATE users SET"));
        assert!(p.contains("active = false"));
        assert!(p.contains("WHERE last_login < \"2020-01-01\""));
    }

    #[test]
    fn test_with_multiple_conditions() {
        let update = SurrealUpdate::table("logs")
            .with_field("archived", true)
            .with_condition(crate::surreal_expr!("level = {}", "debug"))
            .with_condition(crate::surreal_expr!("age > {}", 30i64));

        assert_eq!(
            update.preview(),
            "UPDATE logs SET archived = true WHERE level = \"debug\" AND age > 30"
        );
    }

    #[test]
    fn test_table_constructor() {
        let update = SurrealUpdate::table("products").with_field("in_stock", true);
        let p = update.preview();
        assert!(p.starts_with("UPDATE products SET"));
    }

    #[test]
    fn test_with_arbitrary_target_expression() {
        let upd = SurrealUpdate::new(crate::surreal_expr!("user WHERE active = true"))
            .with_field("checked", true);

        let p = upd.preview();
        assert!(p.contains("UPDATE user WHERE active = true SET"));
    }

    #[test]
    fn test_content_with_condition() {
        let update = SurrealUpdate::table("cache")
            .content()
            .with_field("data", "refreshed".to_string())
            .with_condition(crate::surreal_expr!("expired = {}", true));

        let p = update.preview();
        assert!(p.contains("CONTENT"));
        assert!(p.contains("WHERE expired = true"));
    }

    #[test]
    fn test_merge_with_condition() {
        let update = SurrealUpdate::table("users")
            .merge()
            .with_field("verified", true)
            .with_condition(crate::surreal_expr!("email_confirmed = {}", true));

        let p = update.preview();
        assert!(p.contains("MERGE"));
        assert!(p.contains("WHERE email_confirmed = true"));
    }
}
