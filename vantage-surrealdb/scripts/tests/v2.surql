-- =====================================================
-- EXAMPLE QUERIES
-- =====================================================

-- Get all products for a specific bakery
SELECT * FROM product WHERE bakery = bakery:hill_valley AND is_deleted = false;

-- Get all clients for a bakery
SELECT * FROM client WHERE bakery = bakery:hill_valley;

-- Get all products with their current stock
SELECT name, price, inventory.stock AS stock FROM product WHERE is_deleted = false;

-- Alternative: Select the entire inventory object
SELECT name, price, inventory FROM product WHERE is_deleted = false;

-- Get client orders with embedded line items (with bakery boundary check)
SELECT * FROM client:marty->placed->order WHERE bakery = bakery:hill_valley;

-- Get all products from a bakery (using direct bakery reference)
SELECT * FROM product WHERE bakery = bakery:hill_valley AND is_deleted = false;

-- Get order details with product info (filtered by bakery)
SELECT
    id,
    created_at,
    lines[*].{
        product_name: product.name,
        quantity: quantity,
        price: price,
        subtotal: quantity * price
    } AS items
FROM order
WHERE bakery = bakery:hill_valley;

-- Find which clients ordered a specific product (with bakery boundary check)
SELECT DISTINCT <-placed<-client AS customers
FROM order
WHERE lines.product CONTAINS product:flux_cupcake
AND bakery = bakery:hill_valley;

-- Alternative: Find clients who ordered a specific product (more explicit, with bakery boundary check)
SELECT DISTINCT <-placed<-client AS customers
FROM order
WHERE array::len(lines[WHERE product = product:flux_cupcake]) > 0
AND bakery = bakery:hill_valley;

-- Get low stock products for a specific bakery
SELECT name, inventory.stock
FROM product
WHERE bakery = bakery:hill_valley
AND inventory.stock < 20
AND is_deleted = false;

-- Inventory vs Demand Analytics: Show products and order statistics for a specific bakery
SELECT
    name AS product_name,
    inventory.stock AS current_inventory,
    array::first((SELECT count() FROM order WHERE bakery = bakery:hill_valley GROUP ALL)).count AS total_orders
FROM product
WHERE bakery = bakery:hill_valley AND is_deleted = false;

-- Client Calorie Consumption Analytics: Show total calories ordered by each Hill Valley Bakery client
SELECT
    name AS client_name,
    math::sum(
        SELECT VALUE math::sum(
            array::map(lines, |$line| $line.quantity * $line.product.calories)
        )
        FROM ->placed->order
        WHERE bakery = bakery:hill_valley
    ) AS total_calories_ordered
FROM client
WHERE bakery = bakery:hill_valley
ORDER BY name;
