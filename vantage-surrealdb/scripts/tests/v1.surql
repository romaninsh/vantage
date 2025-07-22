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
