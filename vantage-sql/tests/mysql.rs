#[cfg(feature = "mysql")]
#[macro_use]
#[path = "common/safe_writes.rs"]
mod safe_writes_contract;

#[cfg(all(feature = "mysql", feature = "vista"))]
#[path = "common/dotted.rs"]
mod dotted;

#[cfg(all(feature = "mysql", feature = "vista"))]
#[macro_use]
#[path = "common/vista.rs"]
mod vista_suite;

#[cfg(feature = "mysql")]
mod mysql {
    /// True when the server under test is MariaDB rather than MySQL — for
    /// the few assertions where the two servers legitimately differ.
    pub(crate) async fn is_mariadb(db: &vantage_sql::mysql::MysqlDB) -> bool {
        let version: String = sqlx::query_scalar("SELECT VERSION()")
            .fetch_one(db.pool())
            .await
            .unwrap();
        version.contains("MariaDB")
    }

    #[path = "4_aggregates.rs"]
    mod aggregates;
    #[path = "2_associated.rs"]
    mod associated;
    #[path = "1_chrono.rs"]
    mod chrono;
    #[path = "3_complex_queries.rs"]
    mod complex_queries;
    #[path = "3_complex_queries_my.rs"]
    mod complex_queries_my;
    #[path = "4_conditions.rs"]
    mod conditions;
    #[path = "1_decimal.rs"]
    mod decimal;
    #[path = "2_defer.rs"]
    mod defer;
    #[path = "4_editable_data_set.rs"]
    mod editable_data_set;
    #[path = "2_expressions.rs"]
    mod expressions;
    #[path = "2_identifier.rs"]
    mod identifier;
    #[cfg(feature = "vista")]
    #[path = "9_import.rs"]
    mod import;
    #[cfg(feature = "vista")]
    #[path = "9_import_batches.rs"]
    mod import_batches;
    #[cfg(feature = "vista")]
    #[path = "9_import_conflicts.rs"]
    mod import_conflicts;
    #[path = "2_insert.rs"]
    mod insert;
    #[path = "4_readable_data_set.rs"]
    mod readable_data_set;
    #[path = "2_records.rs"]
    mod records;
    #[path = "5_references.rs"]
    mod references;
    #[path = "9_safe_writes.rs"]
    mod safe_writes;
    #[path = "2_search.rs"]
    mod search;
    #[path = "3_select.rs"]
    mod select;
    #[path = "4_table_def.rs"]
    mod table_def;
    #[path = "1_types_record.rs"]
    mod types_record;
    #[path = "1_types_round_trip.rs"]
    mod types_round_trip;
    #[cfg(feature = "vista")]
    #[path = "6_vista.rs"]
    mod vista;
    #[cfg(feature = "vista")]
    #[path = "6_vista_contract.rs"]
    mod vista_contract;
    #[cfg(feature = "vista")]
    #[path = "6_vista_paging.rs"]
    mod vista_paging;
    #[cfg(all(feature = "vista", feature = "rhai"))]
    #[path = "6_vista_rhai_modify.rs"]
    mod vista_rhai_modify;
    #[cfg(all(feature = "vista", feature = "rhai"))]
    #[path = "6_vista_rhai_refs.rs"]
    mod vista_rhai_refs;
}
