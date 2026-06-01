SELECT
  STRFTIME('%Y-%m', "o"."created_at") AS "month",
  COUNT(DISTINCT "o"."id") AS "orders",
  COALESCE(SUM(("ol"."quantity" * "ol"."price")), 0) AS "revenue"
FROM
  "client_order" AS "o"
  LEFT JOIN "order_line" AS "ol" ON "ol"."order_id" = "o"."id"
GROUP BY
  STRFTIME('%Y-%m', "o"."created_at")
ORDER BY
  MONTH