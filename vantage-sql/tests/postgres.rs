#[cfg(feature = "postgres")]
#[macro_use]
#[path = "common/safe_writes.rs"]
mod safe_writes_contract;

#[macro_use]
#[path = "common/expression_ids.rs"]
mod expression_ids_contract;

#[macro_use]
#[path = "common/relation_aggregates.rs"]
mod relation_aggregates_contract;

#[cfg(all(feature = "postgres", feature = "vista"))]
#[path = "common/dotted.rs"]
mod dotted;

#[cfg(feature = "postgres")]
#[path = "common/condition_text.rs"]
mod condition_text;

#[cfg(all(feature = "postgres", feature = "vista", feature = "rhai"))]
#[path = "common/rule_conditions.rs"]
mod rule_conditions;

#[cfg(all(feature = "postgres", feature = "vista"))]
#[macro_use]
#[path = "common/vista.rs"]
mod vista_suite;

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
    #[cfg(all(feature = "vista", feature = "rhai"))]
    #[path = "7_rule_conditions.rs"]
    mod rule_conditions;
    #[path = "9_expression_ids.rs"]
    mod expression_ids;
    #[path = "5_relation_aggregates.rs"]
    mod relation_aggregates;
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
    #[path = "6_vista_transactions.rs"]
    mod vista_transactions;
}
