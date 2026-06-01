SELECT
  "c"."name" AS "client",
  COALESCE(SUM(("ol"."quantity" * "ol"."price")), 0.0) AS "total_spent",
  COUNT(DISTINCT "o"."id") AS "order_count",
  ROUND(
    (
      CAST(SUM(("ol"."quantity" * "ol"."price")) AS REAL) / NULLIF(COUNT(DISTINCT "o"."id"), 0)
    ),
    2
  ) AS "avg_order"
FROM
  "client" AS "c"
  LEFT JOIN "client_order" AS "o" ON "o"."client_id" = "c"."id"
  LEFT JOIN "order_line" AS "ol" ON "ol"."order_id" = "o"."id"
GROUP BY
  "c"."id",
  "c"."name"
ORDER BY
  total_spent DESC