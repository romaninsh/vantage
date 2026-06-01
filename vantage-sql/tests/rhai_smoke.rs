//! Checkpoint 1+2 tests: engine creation + constructor functions.

#[cfg(feature = "rhai")]
mod rhai_smoke {
    use vantage_expressions::Expressive;
    use vantage_sql::condition::SqliteCondition;
    use vantage_sql::sqlite::statements::SqliteSelect;
    use vantage_sql::sqlite::statements::select::join::SqliteSelectJoin;
    use vantage_sql::sqlite::types::AnySqliteType;

    vantage_sql::register_engine!(
        value: AnySqliteType,
        select: SqliteSelect,
        join: SqliteSelectJoin,
        cond: SqliteCondition,
    );

    pub fn create_engine() -> rhai::Engine {
        __create_engine()
    }

    fn eval_rhai(code: &str) -> rhai::Dynamic {
        let engine = create_engine();
        engine
            .eval(code)
            .unwrap_or_else(|e| panic!("rhai eval failed: {e}"))
    }

    #[test]
    fn engine_creates() {
        let _engine = create_engine();
    }

    #[test]
    fn eval_literal() {
        let result: i64 = eval_rhai("2 + 3").cast();
        assert_eq!(result, 5);
    }

    #[test]
    fn ident_creates_identifier() {
        let result = eval_rhai(r#"ident("name")"#);
        let id = result
            .try_cast::<vantage_sql::rhai_engine::RhaiIdent>()
            .expect("should be RhaiIdent");
        let expr: vantage_expressions::Expression<AnySqliteType> = id.0.expr();
        assert_eq!(expr.preview(), r#""name""#);
    }

    #[test]
    fn ident_dot_of() {
        let result = eval_rhai(r#"ident("name").dot_of("u")"#);
        let id = result
            .try_cast::<vantage_sql::rhai_engine::RhaiIdent>()
            .expect("should be RhaiIdent");
        let expr: vantage_expressions::Expression<AnySqliteType> = id.0.expr();
        assert_eq!(expr.preview(), r#""u"."name""#);
    }

    #[test]
    fn table_and_indexer() {
        let result = eval_rhai(
            r#"
            let t = table("users").alias("u");
            t["name"]
        "#,
        );
        let id = result
            .try_cast::<vantage_sql::rhai_engine::RhaiIdent>()
            .expect("should be RhaiIdent");
        let expr: vantage_expressions::Expression<AnySqliteType> = id.0.expr();
        assert_eq!(expr.preview(), r#""u"."name""#);
    }

    #[test]
    fn table_as_alias() {
        let result = eval_rhai(r#"table("users").alias("u")"#);
        let id = result
            .try_cast::<vantage_sql::rhai_engine::RhaiIdent>()
            .expect("should be RhaiIdent");
        assert_eq!(id.0.alias(), Some("u"));
        assert_eq!(id.0.name(), "users");
    }

    #[test]
    fn expr_as_alias() {
        let result = eval_rhai(r#"expr("COUNT(*)").alias("total")"#);
        let ex = result
            .try_cast::<vantage_sql::rhai_engine::RhaiExpr<AnySqliteType>>()
            .expect("should be RhaiExpr");
        assert_eq!(ex.0.preview(), r#"COUNT(*) AS "total""#);
    }

    #[test]
    fn sum_aggregate() {
        let result = eval_rhai(
            r#"
            let ol = table("order_line").alias("ol");
            sum(ol["price"])
        "#,
        );
        let ex = result
            .try_cast::<vantage_sql::rhai_engine::RhaiExpr<AnySqliteType>>()
            .expect("should be RhaiExpr");
        assert_eq!(ex.0.preview(), r#"SUM("ol"."price")"#);
    }

    #[test]
    fn coalesce_with_mul() {
        let result = eval_rhai(
            r#"
            let ol = table("order_line").alias("ol");
            coalesce(mul(ol["quantity"], ol["price"]), expr("0"))
        "#,
        );
        let ex = result
            .try_cast::<vantage_sql::rhai_engine::RhaiExpr<AnySqliteType>>()
            .expect("should be RhaiExpr");
        assert_eq!(
            ex.0.preview(),
            r#"COALESCE(("ol"."quantity" * "ol"."price"), 0)"#
        );
    }

    // ── Checkpoint 3: Select builder tests ────────────────────────

    type TestSelect = vantage_sql::rhai_engine::RhaiSelect<
        AnySqliteType,
        SqliteSelect,
        SqliteSelectJoin,
        SqliteCondition,
    >;

    #[test]
    fn select_basic() {
        let result = eval_rhai(
            r#"
            select()
                .from("users")
                .field("name")
                .field("email")
        "#,
        );
        let sel = result
            .try_cast::<TestSelect>()
            .expect("should be RhaiSelect");
        assert_eq!(
            sel.inner.preview(),
            r#"SELECT "name", "email" FROM "users""#
        );
    }

    #[test]
    fn select_with_where_order_limit() {
        let result = eval_rhai(
            r#"
            let u = table("users").alias("u");
            select()
                .from(u)
                .expression(expr("{}", [u["name"]]))
                .where(u["salary"] > 50000)
                .order_by(u["name"], "asc")
                .limit(10, 0)
        "#,
        );
        let sel = result
            .try_cast::<TestSelect>()
            .expect("should be RhaiSelect");
        let sql = sel.inner.preview();
        assert!(sql.contains("SELECT"), "sql: {}", sql);
        assert!(sql.contains("FROM"), "sql: {}", sql);
        assert!(sql.contains("WHERE"), "sql: {}", sql);
        assert!(sql.contains("ORDER BY"), "sql: {}", sql);
        assert!(sql.contains("LIMIT"), "sql: {}", sql);
    }

    #[test]
    fn select_with_inner_join() {
        let result = eval_rhai(
            r#"
            let u = table("users").alias("u");
            select()
                .from(u)
                .inner_join("departments", "d",
                    ident("department_id").dot_of("u") == ident("id").dot_of("d"))
                .expression(expr("{}", [u["name"]]))
                .expression(ident("name").dot_of("d").alias("dept"))
        "#,
        );
        let sel = result
            .try_cast::<TestSelect>()
            .expect("should be RhaiSelect");
        let sql = sel.inner.preview();
        assert!(
            sql.contains("INNER JOIN"),
            "should have INNER JOIN: {}",
            sql
        );
        assert!(sql.contains("ON"), "should have ON clause: {}", sql);
    }

    #[test]
    fn select_with_left_join_and_group_by() {
        let result = eval_rhai(
            r#"
            let u = table("users").alias("u");
            let o = table("orders").alias("o");
            select()
                .from(u)
                .left_join("orders", "o",
                    ident("user_id").dot_of("o") == ident("id").dot_of("u"))
                .expression(expr("{}", [u["name"]]))
                .expression(count(o["id"]).alias("order_count"))
                .group_by(u["id"])
                .group_by(u["name"])
                .order_by(expr("order_count"), "desc")
        "#,
        );
        let sel = result
            .try_cast::<TestSelect>()
            .expect("should be RhaiSelect");
        let sql = sel.inner.preview();
        assert!(sql.contains("LEFT JOIN"), "should have LEFT JOIN: {}", sql);
        assert!(sql.contains("GROUP BY"), "should have GROUP BY: {}", sql);
    }

    #[test]
    fn select_distinct() {
        let result = eval_rhai(
            r#"
            select()
                .from("products")
                .distinct()
                .field("category")
        "#,
        );
        let sel = result
            .try_cast::<TestSelect>()
            .expect("should be RhaiSelect");
        let sql = sel.inner.preview();
        assert!(sql.contains("DISTINCT"), "should have DISTINCT: {}", sql);
    }

    #[test]
    fn select_with_having() {
        let result = eval_rhai(
            r#"
            let p = table("products").alias("p");
            select()
                .from(p)
                .field("category")
                .expression(count(expr("*")).alias("cnt"))
                .group_by(p["category"])
                .having(count(expr("*")) > 5)
        "#,
        );
        let sel = result
            .try_cast::<TestSelect>()
            .expect("should be RhaiSelect");
        let sql = sel.inner.preview();
        assert!(sql.contains("HAVING"), "should have HAVING: {}", sql);
    }

    // ── Checkpoint 4: Operator tests ──────────────────────────────

    #[test]
    fn op_ident_eq_int() {
        let result = eval_rhai(
            r#"
            let u = table("users").alias("u");
            u["id"] == 42
        "#,
        );
        let ex = result
            .try_cast::<vantage_sql::rhai_engine::RhaiExpr<AnySqliteType>>()
            .expect("should be RhaiExpr");
        assert_eq!(ex.0.preview(), r#""u"."id" = 42"#);
    }

    #[test]
    fn op_ident_eq_string() {
        let result = eval_rhai(
            r#"
            let u = table("users").alias("u");
            u["role"] == "admin"
        "#,
        );
        let ex = result
            .try_cast::<vantage_sql::rhai_engine::RhaiExpr<AnySqliteType>>()
            .expect("should be RhaiExpr");
        assert_eq!(ex.0.preview(), r#""u"."role" = 'admin'"#);
    }

    #[test]
    fn op_ident_eq_bool() {
        let result = eval_rhai(
            r#"
            let p = table("product").alias("p");
            p["is_deleted"] == false
        "#,
        );
        let ex = result
            .try_cast::<vantage_sql::rhai_engine::RhaiExpr<AnySqliteType>>()
            .expect("should be RhaiExpr");
        assert_eq!(ex.0.preview(), r#""p"."is_deleted" = 0"#);
    }

    #[test]
    fn op_ident_gt_ident() {
        let result = eval_rhai(
            r#"
            let u = table("users").alias("u");
            u["salary"] > u["min_salary"]
        "#,
        );
        let ex = result
            .try_cast::<vantage_sql::rhai_engine::RhaiExpr<AnySqliteType>>()
            .expect("should be RhaiExpr");
        assert_eq!(ex.0.preview(), r#""u"."salary" > "u"."min_salary""#);
    }

    #[test]
    fn op_ident_ne_float() {
        let result = eval_rhai(
            r#"
            let p = table("product").alias("p");
            p["price"] != 0.0
        "#,
        );
        let ex = result
            .try_cast::<vantage_sql::rhai_engine::RhaiExpr<AnySqliteType>>()
            .expect("should be RhaiExpr");
        assert_eq!(ex.0.preview(), r#""p"."price" != 0.0"#);
    }

    #[test]
    fn op_ident_le_int() {
        let result = eval_rhai(
            r#"
            let u = table("users").alias("u");
            u["age"] <= 65
        "#,
        );
        let ex = result
            .try_cast::<vantage_sql::rhai_engine::RhaiExpr<AnySqliteType>>()
            .expect("should be RhaiExpr");
        assert_eq!(ex.0.preview(), r#""u"."age" <= 65"#);
    }
}
