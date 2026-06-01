SELECT
  TO_CHAR("o"."created_at", 'YYYY-MM') AS "month",
  COUNT(DISTINCT "o"."id") AS "orders",
  COALESCE(SUM(("ol"."quantity" * "ol"."price")), 0) AS "revenue"
FROM
  "client_order" AS "o"
  LEFT JOIN "order_line" AS "ol" ON "ol"."order_id" = "o"."id"
GROUP BY
  TO_CHAR("o"."created_at", 'YYYY-MM')
ORDER BY
  MONTH