mod sqlite {
    #[path = "1_types_round_trip.rs"]
    mod types_round_trip;
    #[path = "1_types_record.rs"]
    mod types_record;
    #[path = "2_expressions.rs"]
    mod expressions;
    #[path = "2_insert.rs"]
    mod insert;
    mod bakery;
    mod statements;
}
