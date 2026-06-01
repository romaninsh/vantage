// Q5: Client Balance Report (paying vs non-paying)
// Features: CASE grouping, COUNT/SUM/AVG aggregates, GROUP BY

let c = table("client").alias("c");

select()
    .from(c)
    .expression(
        case_when()
            .when(c["is_paying_client"] == true, expr("'Paying'"))
            .else_(expr("'Non-Paying'"))
            .expr()
            .alias("client_type"))
    .expression(count(c["id"]).alias("client_count"))
    .expression(sum(c["balance"]).alias("total_balance"))
    .expression(avg(c["balance"]).alias("avg_balance"))
    .group_by(c["is_paying_client"])
