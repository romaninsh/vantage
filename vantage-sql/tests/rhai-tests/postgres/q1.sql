SELECT
  "u"."id",
  "u"."name",
  SUM("u"."salary") AS "total_salary",
  AVG("u"."age") AS "avg_age"
FROM
  "users" AS "u"
WHERE
  "u"."active" = TRUE
  AND "u"."department_id" != 0
GROUP BY
  "u"."id",
  "u"."name"
HAVING
  SUM("u"."salary") > 50000
ORDER BY
  "u"."name",
  total_salary DESC
LIMIT
  10 OFFSET 0