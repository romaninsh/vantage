SELECT
  "p"."name" AS "product",
  "p"."inventory_stock" AS "stock",
  COALESCE(SUM("ol"."quantity"), 0) AS "units_sold",
  CASE
    WHEN "p"."inventory_stock" <= 5 THEN 'CRITICAL'
    WHEN "p"."inventory_stock" <= 15 THEN 'LOW'
    WHEN "p"."inventory_stock" <= 30 THEN 'OK'
    ELSE 'PLENTY'
  END AS "stock_status",
  CASE
    WHEN "p"."is_deleted" = 0 THEN 'active'
    ELSE 'DELETED'
  END AS "active"
FROM
  "product" AS "p"
  LEFT JOIN "order_line" AS "ol" ON "ol"."product_id" = "p"."id"
GROUP BY
  "p"."id",
  "p"."name",
  "p"."inventory_stock",
  "p"."is_deleted"
ORDER BY
  stock