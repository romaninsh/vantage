SELECT
  CASE
    WHEN "c"."is_paying_client" = TRUE THEN 'Paying'
    ELSE 'Non-Paying'
  END AS "client_type",
  COUNT("c"."id") AS "client_count",
  SUM("c"."balance") AS "total_balance",
  AVG("c"."balance") AS "avg_balance"
FROM
  "client" AS "c"
GROUP BY
  "c"."is_paying_client"