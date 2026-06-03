-- =====================================================
-- VANTAGE SURREALDB — 10 COMPLEX QUERIES
-- Exercises advanced SurrealQL features against v3 seed
-- =====================================================

-- Query 01: Department org chart via recursive graph traversal
-- Features: graph edges (<-sub_dept<-), recursive traversal (..), record link traversal
-- Shows the full hierarchy with root detection via parent record link
SELECT
    name,
    bakery.name AS bakery,
    budget,
    parent.name AS parent_dept,
    IF parent = NONE THEN 'root' ELSE parent.name END AS hierarchy_level,
    ->sub_dept->department.name AS children
FROM department
ORDER BY bakery.name, parent.name, name;

-- Query 02: Order details with FETCH + embedded document traversal
-- Features: FETCH (eagerly resolve record links), array::map (transform embedded docs),
-- computed fields (subtotal), record link field access (client.name, product.name)
-- No JOINs needed — SurrealDB resolves links transparently
SELECT
    id,
    client.name AS client_name,
    bakery.name AS bakery_name,
    status,
    created_at,
    array::map(lines, |$l| {
        product_name: $l.product.name,
        quantity: $l.quantity,
        price: $l.price,
        subtotal: $l.quantity * $l.price
    }) AS items,
    total AS order_total
FROM order
ORDER BY created_at DESC
FETCH client, bakery, lines.product;

-- Query 03: Product-level sales stats by aggregating embedded line items
-- Features: subquery with SPLIT (unnest array), outer GROUP BY,
-- math::sum aggregate, record link field access on split results
-- SurrealDB 3.x disallows SPLIT + GROUP in same SELECT, so we use
-- a subquery to SPLIT first, then GROUP the result
SELECT
    line.product.name AS product_name,
    line.product.category AS category,
    math::sum(line.quantity) AS total_qty_sold,
    math::sum(line.quantity * line.price) AS total_revenue,
    count() AS times_ordered
FROM (
    SELECT lines AS line FROM order SPLIT lines
)
GROUP BY line.product, line.product.name, line.product.category
ORDER BY total_revenue DESC;

-- Query 04: Employee salary banding with IF/ELSE expressions
-- Features: IF/ELSE as expression (SurrealQL equivalent of CASE/WHEN),
-- record link traversal (department.name, bakery.name),
-- time::now() for computed tenure
-- IF/ELSE is a first-class expression in SurrealQL, no CASE keyword needed
SELECT
    name,
    salary,
    department.name AS department,
    bakery.name AS bakery,
    role,
    IF salary >= 100000 THEN 'senior'
    ELSE IF salary >= 80000 THEN 'mid'
    ELSE IF salary >= 60000 THEN 'junior'
    ELSE 'intern' END AS band,
    time::format(hire_date, '%Y-%m-%d') AS hire_date,
    math::floor(time::unix(time::now()) - time::unix(hire_date)) / 86400 / 365 AS years_tenure
FROM employee
ORDER BY salary DESC;

-- Query 05: Client segmentation with array ops and record links
-- Features: CONTAINSALL (array subset check), array::map (transform),
-- array::distinct (deduplicate), subquery as computed column,
-- graph traversal (->primary_bakery->bakery)
-- Demonstrates SurrealDB's array operators for set-based filtering
SELECT
    name,
    email,
    is_paying_client,
    balance,
    bakery.name AS primary_bakery,
    array::distinct(
        SELECT VALUE bakery.name FROM order WHERE client = $parent.id
    ) AS bakeries_ordered,
    array::len(SELECT id FROM order WHERE client = $parent.id) AS order_count,
    math::sum(SELECT VALUE total FROM order WHERE client = $parent.id) AS total_spent,
    IF is_paying_client = true
       AND array::len(SELECT id FROM order WHERE client = $parent.id) >= 2
    THEN 'premium'
    ELSE IF is_paying_client = true THEN 'regular'
    ELSE 'basic' END AS segment
FROM client
ORDER BY total_spent DESC;

-- Query 06: Monthly revenue per bakery with time functions
-- Features: time::format (date formatting), GROUP BY on idiom field,
-- math::sum/math::mean aggregates, time-based filtering, subquery pattern
-- SurrealDB 3.x GROUP BY requires idioms (not function calls), so we
-- pre-compute the month in a subquery then GROUP on the result
SELECT
    month,
    bakery,
    count() AS order_count,
    math::sum(total) AS monthly_revenue,
    math::mean(total) AS avg_order_value,
    array::distinct(status) AS statuses
FROM (
    SELECT
        time::format(created_at, '%Y-%m') AS month,
        bakery.name AS bakery,
        total,
        status
    FROM order
    WHERE created_at >= d'2025-01-01T00:00:00Z'
)
GROUP BY month, bakery
ORDER BY month DESC, bakery;

-- Query 07: DEFINE FUNCTION + LET variables + closures for bonus calculation
-- Features: DEFINE FUNCTION (custom SurrealQL function), LET variables,
-- closure/lambda syntax (|$a, $b| ...), IF/ELSE in function body,
-- time::now() arithmetic, math::floor
-- Demonstrates SurrealDB's scripting capabilities — no PL/pgSQL needed
-- NOTE: Single-expression body to avoid multi-statement parse issues in --multi mode
DEFINE FUNCTION IF NOT EXISTS fn::calculate_bonus($salary: decimal, $rating: int, $hire_date: datetime) -> decimal {
    RETURN math::floor($salary * ((IF $rating >= 5 THEN 0.15 ELSE IF $rating >= 4 THEN 0.10 ELSE IF $rating >= 3 THEN 0.05 ELSE 0.0 END) + (IF math::floor((time::unix(time::now()) - time::unix($hire_date)) / 86400 / 365) >= 3 THEN 0.05 ELSE IF math::floor((time::unix(time::now()) - time::unix($hire_date)) / 86400 / 365) >= 1 THEN 0.02 ELSE 0.0 END)))
};
SELECT
    name,
    salary,
    performance_rating AS rating,
    department.name AS department,
    bakery.name AS bakery,
    fn::calculate_bonus(salary, performance_rating, hire_date) AS bonus
FROM employee
ORDER BY bonus DESC;

-- Query 08: Product tag frequency via SPLIT (unnest) + outer GROUP BY
-- Features: SPLIT (unnest array field), subquery pattern,
-- count() aggregate, array::group (collect into array)
-- SurrealDB 3.x disallows SPLIT + GROUP in same SELECT
SELECT
    tag,
    count() AS product_count,
    array::group(product_name) AS products
FROM (
    SELECT tags AS tag, name AS product_name FROM product SPLIT tags
)
GROUP BY tag
ORDER BY product_count DESC, tag;

-- Query 09: Supplier cost comparison via graph edge traversal
-- Features: querying FROM edge table (supply), record link traversal
-- (supplier.name, product.name, product.bakery.name), edge data access
-- (unit_cost, lead_time_days), computed margin and margin percentage
-- SurrealDB edges created by RELATE carry data AND link records.
-- The `supplier` and `product` fields on each edge are record links,
-- so `.name` traverses the link to fetch the related record's name.
-- No JOINs — one edge table connects supplier→product with cost data.
SELECT
    supplier.name AS supplier_name,
    product.name AS product_name,
    product.bakery.name AS bakery,
    product.price AS retail_price,
    unit_cost,
    lead_time_days,
    product.price - unit_cost AS margin,
    math::round((product.price - unit_cost) / product.price * 100) AS margin_pct
FROM supply
ORDER BY product_name, unit_cost;

-- Query 10: Full-text search with BM25 scoring and highlights
-- Features: @1@ (full-text match operator with predicate reference),
-- search::score() (BM25 relevance), search::highlight() (match highlighting),
-- multi-term search, combined with computed price categorization
-- Demonstrates SurrealDB's built-in full-text search — no extension needed
SELECT
    name,
    category,
    price,
    IF price >= 200 THEN 'premium' ELSE IF price >= 100 THEN 'mid' ELSE 'budget' END AS price_tier,
    search::score(1) AS relevance,
    search::highlight('<mark>', '</mark>', 1) AS highlighted_name
FROM product
WHERE name @1@ 'cupcake chocolate cake bread'
ORDER BY relevance DESC;
