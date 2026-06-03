-- =====================================================
-- COMPLEX QUERIES (v4)
-- =====================================================
-- Ten queries against the v4 seed, each leaning on an advanced SurrealQL
-- feature that has no plain-SQL equivalent. Run with ./test.sh (loads via
-- ingress.sh into database `v4`). Every query is a single statement and
-- returns rows.

-- Query 01: Graph traversal + null-coalescing
-- Features: forward edge hop `->reports_to->employee`, record-link field
-- dereference `department.name`, array::first over a traversal, and the
-- `??` operator to default the owner (who has no manager) to 'n/a'.
SELECT
    name,
    title,
    salary,
    department.name AS department,
    array::first(->reports_to->employee.name) ?? 'n/a' AS manager
FROM employee
ORDER BY salary DESC;

-- Query 02: Recursive graph traversal
-- Features: ranged recursion `@.{1..5}(<-reports_to<-employee)` walks the
-- org chart downward and returns the records at the *deepest* reachable
-- level — i.e. each manager's most-junior report(s). `count(<-reports_to)`
-- keeps only people who actually manage someone.
SELECT
    name AS manager,
    @.{1..5}(<-reports_to<-employee).name AS deepest_report
FROM employee
WHERE count(<-reports_to) > 0
ORDER BY name;

-- Query 03: GROUP BY with the math:: statistics family
-- Features: grouping by a record-link field, then count(), math::sum,
-- math::mean, math::max and a rounded math::stddev to show salary spread
-- per department.
SELECT
    department.name AS department,
    count() AS headcount,
    math::sum(salary) AS payroll,
    math::mean(salary) AS avg_salary,
    math::max(salary) AS top_salary,
    math::round(math::stddev(salary)) AS salary_spread
FROM employee
GROUP BY department
ORDER BY payroll DESC;

-- Query 04: Correlated subquery with $parent
-- Features: count()/math::sum over a graph hop `->placed->order.total`
-- (where `total` is a COMPUTED field on order), plus a correlated subquery
-- that re-enters the order table filtered by `client = $parent.id` to pull
-- each client's single biggest order.
SELECT
    name,
    tier,
    count(->placed->order) AS num_orders,
    math::sum(->placed->order.total) AS lifetime_spend,
    (SELECT VALUE math::max(total) FROM order WHERE client = $parent.id GROUP ALL)[0] AS biggest_order
FROM client
WHERE count(->placed->order) > 0
ORDER BY lifetime_spend DESC;

-- Query 05: Closures over embedded arrays
-- Features: anonymous functions `|$l| ...` driving array::map (build a
-- per-line breakdown object) and array::fold (accumulate the order total)
-- over the embedded `lines` array.
SELECT
    id,
    array::len(lines) AS line_count,
    lines.map(|$l| { product: $l.product.name, subtotal: $l.quantity * $l.price }) AS breakdown,
    lines.fold(0, |$acc, $l| $acc + ($l.quantity * $l.price)) AS computed_total
FROM order
ORDER BY computed_total DESC
LIMIT 5;

-- Query 06: Aggregating graph-edge properties
-- Features: the rating lives ON the `reviewed` edge, so `<-reviewed.rating`
-- reads a property off the relation itself; math::mean/max/min then turn the
-- per-product review edges into an aggregate score.
SELECT
    name,
    category,
    count(<-reviewed) AS num_reviews,
    math::mean(<-reviewed.rating) AS avg_rating,
    math::max(<-reviewed.rating) AS best,
    math::min(<-reviewed.rating) AS worst
FROM product
WHERE count(<-reviewed) > 0
ORDER BY avg_rating DESC;

-- Query 07: Time bucketing
-- Features: time::group() buckets each order's datetime into a calendar
-- month, GROUP BY then rolls up order count, summed revenue and a rounded
-- mean order value across the year.
SELECT
    time::group(created_at, 'month') AS month,
    count() AS orders,
    math::sum(total) AS revenue,
    math::round(math::mean(total)) AS avg_order_value
FROM order
GROUP BY month
ORDER BY month;

-- Query 08: Fuzzy string matching
-- Features: string::similarity::jaro_winkler scores each client name
-- against a deliberately misspelled search term ('marti mcfligh'),
-- string::lowercase normalises case, and string::words splits the name into
-- tokens. Ranked best match first.
SELECT
    name,
    string::similarity::jaro_winkler(string::lowercase(name), 'marti mcfligh') AS similarity,
    string::words(name) AS name_parts
FROM client
ORDER BY similarity DESC
LIMIT 4;

-- Query 09: Object functions
-- Features: object::entries explodes the nutrition object into key/value
-- pairs and object::values feeds math::sum to total the macros, all while
-- filtering on a nested field `nutrition.sugar`.
SELECT
    name,
    category,
    object::entries(nutrition) AS facts,
    math::sum(object::values(nutrition)) AS total_macros,
    nutrition.sugar AS sugar
FROM product
WHERE nutrition.sugar > 15
ORDER BY sugar DESC;

-- Query 10: SPLIT in a subquery + conditional banding
-- Features: SPLIT and GROUP are mutually exclusive, so the inner query
-- SPLITs each product into one row per tag and the outer query GROUPs by
-- tag; a nested IF/THEN/ELSE expression then bands each tag by mean price.
SELECT
    tag,
    count() AS product_count,
    math::round(math::mean(price)) AS avg_price,
    IF math::mean(price) >= 250 THEN 'premium' ELSE IF math::mean(price) >= 150 THEN 'mid' ELSE 'value' END AS price_band
FROM (SELECT tags AS tag, price FROM product SPLIT tags)
GROUP BY tag
ORDER BY product_count DESC, tag;
