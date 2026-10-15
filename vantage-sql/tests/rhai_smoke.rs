//! Checkpoint 1+2 tests: engine creation + constructor functions.

#[cfg(feature = "rhai")]
mod rhai_smoke {
    use vantage_expressions::Expressive;
    use vantage_rhai::{Block, Env, Host};
    use vantage_sql::condition::SqliteCondition;
    use vantage_sql::sqlite::statements::SqliteSelect;
    use vantage_sql::sqlite::statements::select::join::SqliteSelectJoin;
    use vantage_sql::sqlite::types::AnySqliteType;

    // `register_engine!` imports `rhai` into this scope.
    vantage_sql::register_engine!(
        value: AnySqliteType,
        select: SqliteSelect,
        join: SqliteSelectJoin,
        cond: SqliteCondition,
    );

    pub fn host() -> &'static Host {
        __host()
    }

    fn eval_rhai(code: &str) -> rhai::Dynamic {
        host()
            .compile(&Block::from(code))
            .and_then(|script| script.eval(&Env::new()))
            .unwrap_or_else(|e| panic!("rhai eval failed: {e}"))
    }

    #[test]
    fn eval_literal() {
        let result: i64 = eval_rhai("2 + 3").cast();
        assert_eq!(result, 5);
    }

    #[test]
    fn ident_creates_identifier() {
        let result = eval_rhai(r#"sql::ident("name")"#);
        let id = result
            .try_cast::<vantage_sql::rhai_engine::RhaiIdent>()
            .expect("should be RhaiIdent");
        let expr: vantage_expressions::Expression<AnySqliteType> = id.0.expr();
        assert_eq!(expr.preview(), r#""name""#);
    }

    #[test]
    fn ident_dot_of() {
        let result = eval_rhai(r#"sql::ident("name").dot_of("u")"#);
        let id = result
            .try_cast::<vantage_sql::rhai_engine::RhaiIdent>()
            .expect("should be RhaiIdent");
        let expr: vantage_expressions::Expression<AnySqliteType> = id.0.expr();
        assert_eq!(expr.preview(), r#""u"."name""#);
    }

    /// The free functions answer only under `sql::`; `table` is no alias for
    /// `ident` any more, so it stays free for the data vocabulary.
    #[test]
    fn free_functions_need_the_namespace() {
        for (code, name) in [
            (r#"ident("users")"#, "ident"),
            (r#"table("users")"#, "table"),
            (r#"sql::table("users")"#, "sql::table"),
            ("select()", "select"),
        ] {
            let err = host()
                .compile(&Block::from(code))
                .and_then(|script| script.eval(&Env::new()))
                .expect_err(code)
                .to_string();
            assert!(
                err.contains("Function not found") && err.contains(name),
                "{code}: {err}"
            );
        }
    }

    #[test]
    fn ident_and_indexer() {
        let result = eval_rhai(
            r#"
            let t = sql::ident("users").alias("u");
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
    fn ident_as_alias() {
        let result = eval_rhai(r#"sql::ident("users").alias("u")"#);
        let id = result
            .try_cast::<vantage_sql::rhai_engine::RhaiIdent>()
            .expect("should be RhaiIdent");
        assert_eq!(id.0.alias(), Some("u"));
        assert_eq!(id.0.name(), "users");
    }

    #[test]
    fn expr_as_alias() {
        let result = eval_rhai(r#"sql::expr("COUNT(*)").alias("total")"#);
        let ex = result
            .try_cast::<vantage_sql::rhai_engine::RhaiExpr<AnySqliteType>>()
            .expect("should be RhaiExpr");
        assert_eq!(ex.0.preview(), r#"COUNT(*) AS "total""#);
    }

    #[test]
    fn sum_aggregate() {
        let result = eval_rhai(
            r#"
            let ol = sql::ident("order_line").alias("ol");
            sql::sum(ol["price"])
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
            let ol = sql::ident("order_line").alias("ol");
            sql::coalesce(sql::mul(ol["quantity"], ol["price"]), sql::expr("0"))
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

    /// Postgres has no `round(numeric, bigint)`, so the decimals can't be a
    /// bound i64.
    #[test]
    fn round_writes_decimals_into_the_sql() {
        fn has_scalar(e: &vantage_expressions::Expression<AnySqliteType>) -> bool {
            e.parameters.iter().any(|p| match p {
                vantage_expressions::ExpressiveEnum::Scalar(_) => true,
                vantage_expressions::ExpressiveEnum::Nested(n) => has_scalar(n),
                _ => false,
            })
        }
        let ex = eval_rhai(r#"sql::round(sql::sum(sql::ident("spend")), 2)"#)
            .try_cast::<vantage_sql::rhai_engine::RhaiExpr<AnySqliteType>>()
            .expect("should be RhaiExpr");
        assert_eq!(ex.0.preview(), r#"ROUND(SUM("spend"), 2)"#);
        assert!(!has_scalar(&ex.0), "decimals were bound: {:?}", ex.0);
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
            sql::select()
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
            let u = sql::ident("users").alias("u");
            sql::select()
                .from(u)
                .expression(sql::expr("{}", [u["name"]]))
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
            let u = sql::ident("users").alias("u");
            sql::select()
                .from(u)
                .inner_join("departments", "d",
                    sql::ident("department_id").dot_of("u") == sql::ident("id").dot_of("d"))
                .expression(sql::expr("{}", [u["name"]]))
                .expression(sql::ident("name").dot_of("d").alias("dept"))
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
            let u = sql::ident("users").alias("u");
            let o = sql::ident("orders").alias("o");
            sql::select()
                .from(u)
                .left_join("orders", "o",
                    sql::ident("user_id").dot_of("o") == sql::ident("id").dot_of("u"))
                .expression(sql::expr("{}", [u["name"]]))
                .expression(sql::count(o["id"]).alias("order_count"))
                .group_by(u["id"])
                .group_by(u["name"])
                .order_by(sql::expr("order_count"), "desc")
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
            sql::select()
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
            let p = sql::ident("products").alias("p");
            sql::select()
                .from(p)
                .field("category")
                .expression(sql::count(sql::expr("*")).alias("cnt"))
                .group_by(p["category"])
                .having(sql::count(sql::expr("*")) > 5)
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
            let u = sql::ident("users").alias("u");
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
            let u = sql::ident("users").alias("u");
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
            let p = sql::ident("product").alias("p");
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
            let u = sql::ident("users").alias("u");
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
            let p = sql::ident("product").alias("p");
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
            let u = sql::ident("users").alias("u");
            u["age"] <= 65
        "#,
        );
        let ex = result
            .try_cast::<vantage_sql::rhai_engine::RhaiExpr<AnySqliteType>>()
            .expect("should be RhaiExpr");
        assert_eq!(ex.0.preview(), r#""u"."age" <= 65"#);
    }
}
