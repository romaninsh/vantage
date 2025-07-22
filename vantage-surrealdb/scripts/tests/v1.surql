SELECT name, price, inventory.stock FROM product WHERE is_deleted = false;

-- Get client orders with embedded line items
SELECT * FROM client:marty->placed->order;

-- Get all products from a bakery
SELECT * FROM bakery:hill_valley->owns->product WHERE is_deleted = false;

-- Get order details with product info
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

-- Find which clients ordered a specific product
SELECT DISTINCT <-placed<-client AS customers
FROM order
WHERE lines[*].product CONTAINS product:flux_cupcake;

-- Get low stock products
SELECT name, inventory.stock
FROM product
WHERE inventory.stock < 20 AND is_deleted = false;

-- Inventory vs Demand Analytics: Show products where demand exceeds inventory (v1 schema)
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

-- Client Calorie Consumption Analytics: Show total calories ordered by each client
SELECT
    name AS client_name,
    email AS client_email,
    math::sum(
        SELECT VALUE math::sum(
            array::map(lines, |$line| $line.quantity * $line.product.calories)
        )
        FROM ->placed->order
    ) AS total_calories_ordered
FROM client
ORDER BY name;
