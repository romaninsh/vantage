SELECT
  COALESCE("p"."sticker", 'uncategorized') AS "category",
  COUNT("p"."id") AS "product_count",
  SUM("p"."inventory_stock") AS "total_stock",
  AVG("p"."price") AS "avg_price",
  SUM("p"."calories") AS "total_calories"
FROM
  "product" AS "p"
WHERE
  "p"."is_deleted" = false
GROUP BY
  "p"."sticker"
HAVING
  COUNT("p"."id") > 1
ORDER BY
  total_stock DESC