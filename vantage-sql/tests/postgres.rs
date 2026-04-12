#[cfg(feature = "postgres")]
mod postgres {
    #[path = "4_aggregates.rs"]
    mod aggregates;
    #[path = "2_associated.rs"]
    mod associated;
    #[path = "1_chrono.rs"]
    mod chrono;
    #[path = "3_complex_queries.rs"]
    mod complex_queries;
    #[path = "3_complex_queries_pg.rs"]
    mod complex_queries_pg;
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
    #[path = "2_insert.rs"]
    mod insert;
    #[path = "4_readable_data_set.rs"]
    mod readable_data_set;
    #[path = "2_records.rs"]
    mod records;
    #[path = "5_references.rs"]
    mod references;
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
}
