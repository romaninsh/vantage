SELECT
  "c"."name" AS "client",
  COUNT(DISTINCT "o"."id") AS "total_orders",
  GROUP_CONCAT(DISTINCT "p"."name", ',') AS "products_bought",
  COALESCE(SUM(("ol"."quantity" * "ol"."price")), 0) AS "lifetime_value"
FROM
  "client" AS "c"
  LEFT JOIN "client_order" AS "o" ON "o"."client_id" = "c"."id"
  LEFT JOIN "order_line" AS "ol" ON "ol"."order_id" = "o"."id"
  LEFT JOIN "product" AS "p" ON "p"."id" = "ol"."product_id"
GROUP BY
  "c"."id",
  "c"."name"
ORDER BY
  lifetime_value DESC