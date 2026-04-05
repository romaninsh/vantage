mod sqlite {
    #[path = "1_types_round_trip.rs"]
    mod types_round_trip;
    #[path = "1_types_record.rs"]
    mod types_record;
    #[path = "2_expressions.rs"]
    mod expressions;
    #[path = "2_insert.rs"]
    mod insert;
    #[path = "2_defer.rs"]
    mod defer;
    #[path = "2_records.rs"]
    mod records;
    #[path = "2_associated.rs"]
    mod associated;
    mod bakery;
    mod statements;
}
