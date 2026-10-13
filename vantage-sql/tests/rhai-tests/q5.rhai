// Q5: Client Balance Report (paying vs non-paying)
// Features: CASE grouping, COUNT/SUM/AVG aggregates, GROUP BY

let c = sql::ident("client").alias("c");

sql::select()
    .from(c)
    .expression(
        sql::case_when()
            .when(c["is_paying_client"] == true, sql::expr("'Paying'"))
            .else_(sql::expr("'Non-Paying'"))
            .expr()
            .alias("client_type"))
    .expression(sql::count(c["id"]).alias("client_count"))
    .expression(sql::sum(c["balance"]).alias("total_balance"))
    .expression(sql::avg(c["balance"]).alias("avg_balance"))
    .group_by(c["is_paying_client"])
