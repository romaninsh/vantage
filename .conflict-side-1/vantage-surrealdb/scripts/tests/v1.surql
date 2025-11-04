-- =====================================================
-- EXAMPLE QUERIES
-- =====================================================

-- Query 01: Get all products for a specific bakery
SELECT * FROM bakery:hill_valley->owns->product WHERE is_deleted = false order by name ;

-- Query 02: Get all clients for a bakery
SELECT * FROM bakery:hill_valley<-belongs_to<-client order by name;

-- Query 03: Get all products with their current stock
SELECT name, price, inventory.stock AS stock FROM product WHERE is_deleted = false;

-- Query 04: Alternative: Select the entire inventory object
RETURN math::sum(
    (SELECT VALUE inventory.stock FROM product WHERE is_deleted = false)
)-count(SELECT id FROM product);

-- Query 05: Get client orders with embedded line items
SELECT * FROM client:marty->placed->order;

-- Query 06: Get all products from a bakery (using graph relationships)
SELECT * FROM bakery:hill_valley->owns->product WHERE is_deleted = false order by name;

-- Query 07: Get order details with product info
SELECT
    id,
    created_at,
    lines[*].{
        product_name: product.name,
        quantity: quantity,
        price: price,
        subtotal: quantity * price
    } AS items
FROM order;

-- Query 08: Find which clients ordered a specific product
SELECT DISTINCT <-placed<-client AS customers
FROM order
WHERE lines.product CONTAINS product:flux_cupcake;

-- Query 09: Alternative: Find clients who ordered a specific product (more explicit)
SELECT DISTINCT <-placed<-client AS customers
FROM order
WHERE array::len(lines[WHERE product = product:flux_cupcake]) > 0;

-- Query 10: Get low stock products for a specific bakery
SELECT name, inventory.stock
FROM product
WHERE bakery:hill_valley IN <-owns<-bakery
AND inventory.stock < 20
AND is_deleted = false;

-- Query 11: Inventory vs Demand Analytics: Show products where demand exceeds inventory (v1 schema)
SELECT * FROM (
    SELECT
        name AS product_name,
        inventory.stock AS current_inventory,
        math::sum(
            SELECT VALUE math::sum(
                lines[WHERE product = $parent.id].quantity
            )
            FROM order
            WHERE lines.product CONTAINS $parent.id
        ) AS total_items_ordered
    FROM product
    WHERE bakery:hill_valley IN <-owns<-bakery
        AND is_deleted = false
) WHERE total_items_ordered > current_inventory
ORDER BY product_name;

-- Query 12: Client Calorie Consumption Analytics: Show total calories ordered by each client
SELECT
    name AS client_name,
    math::sum(
        SELECT VALUE math::sum(
            array::map(lines, |$line| $line.quantity * $line.product.calories)
        )
        FROM ->placed->order
    ) AS total_calories_ordered
FROM client
ORDER BY name;
