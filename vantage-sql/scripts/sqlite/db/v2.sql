-- Hill Valley Bakery Database - SQLite Version
-- Translated from SurrealDB v2.surql with these adaptations:
-- - Text primary keys matching SurrealDB named IDs (e.g. 'hill_valley', 'marty')
-- - Foreign keys instead of record links
-- - Separate order_line table instead of embedded array<object>
-- - No graph relationships (belongs_to, placed) — use foreign keys
-- - DECIMAL → REAL (SQLite has no native decimal)
-- - DATETIME → TEXT in ISO 8601 format

-- =====================================================
-- SCHEMA DEFINITIONS
-- =====================================================

CREATE TABLE IF NOT EXISTS bakery (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    profit_margin INTEGER NOT NULL CHECK (profit_margin >= 0 AND profit_margin <= 100)
);

CREATE TABLE IF NOT EXISTS client (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    contact_details TEXT NOT NULL,
    is_paying_client BOOLEAN NOT NULL DEFAULT 0,
    balance REAL NOT NULL DEFAULT 0,
    bakery_id TEXT NOT NULL REFERENCES bakery(id)
);

CREATE TABLE IF NOT EXISTS product (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    calories INTEGER NOT NULL CHECK (calories >= 0),
    price INTEGER NOT NULL CHECK (price > 0),
    bakery_id TEXT NOT NULL REFERENCES bakery(id),
    is_deleted BOOLEAN NOT NULL DEFAULT 0,
    inventory_stock INTEGER NOT NULL DEFAULT 0
);

-- "order" is a reserved word in SQL, so we use "client_order"
CREATE TABLE IF NOT EXISTS client_order (
    id TEXT PRIMARY KEY,
    bakery_id TEXT NOT NULL REFERENCES bakery(id),
    client_id TEXT NOT NULL REFERENCES client(id),
    is_deleted BOOLEAN NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS order_line (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id TEXT NOT NULL REFERENCES client_order(id),
    product_id TEXT NOT NULL REFERENCES product(id),
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    price INTEGER NOT NULL CHECK (price > 0)
);

-- =====================================================
-- DATA INSERTION
-- =====================================================

-- Bakery
INSERT INTO bakery (id, name, profit_margin)
VALUES ('hill_valley', 'Hill Valley Bakery', 15);

-- Clients
INSERT INTO client (id, name, email, contact_details, is_paying_client, balance, bakery_id)
VALUES ('marty', 'Marty McFly', 'marty@gmail.com', '555-1955', 1, 150.00, 'hill_valley');

INSERT INTO client (id, name, email, contact_details, is_paying_client, balance, bakery_id)
VALUES ('doc', 'Doc Brown', 'doc@brown.com', '555-1885', 1, 500.50, 'hill_valley');

INSERT INTO client (id, name, email, contact_details, is_paying_client, balance, bakery_id)
VALUES ('biff', 'Biff Tannen', 'biff-3293@hotmail.com', '555-1955', 0, -50.25, 'hill_valley');

-- Products
INSERT INTO product (id, name, calories, price, bakery_id, is_deleted, inventory_stock)
VALUES ('flux_cupcake', 'Flux Capacitor Cupcake', 300, 120, 'hill_valley', 0, 50);

INSERT INTO product (id, name, calories, price, bakery_id, is_deleted, inventory_stock)
VALUES ('delorean_donut', 'DeLorean Doughnut', 250, 135, 'hill_valley', 0, 30);

INSERT INTO product (id, name, calories, price, bakery_id, is_deleted, inventory_stock)
VALUES ('time_tart', 'Time Traveler Tart', 200, 220, 'hill_valley', 0, 20);

INSERT INTO product (id, name, calories, price, bakery_id, is_deleted, inventory_stock)
VALUES ('sea_pie', 'Enchantment Under the Sea Pie', 350, 299, 'hill_valley', 0, 15);

INSERT INTO product (id, name, calories, price, bakery_id, is_deleted, inventory_stock)
VALUES ('hover_cookies', 'Hoverboard Cookies', 150, 199, 'hill_valley', 0, 40);

-- Orders (with client_id from the RELATE statements in SurrealDB)
INSERT INTO client_order (id, bakery_id, client_id, is_deleted, created_at)
VALUES ('order1', 'hill_valley', 'marty', 0, datetime('now'));

INSERT INTO client_order (id, bakery_id, client_id, is_deleted, created_at)
VALUES ('order2', 'hill_valley', 'doc', 0, datetime('now'));

INSERT INTO client_order (id, bakery_id, client_id, is_deleted, created_at)
VALUES ('order3', 'hill_valley', 'doc', 0, datetime('now'));

-- Order lines (flattened from embedded arrays in SurrealDB)
-- Order 1: Marty's order
INSERT INTO order_line (order_id, product_id, quantity, price)
VALUES ('order1', 'flux_cupcake', 3, 120);

INSERT INTO order_line (order_id, product_id, quantity, price)
VALUES ('order1', 'delorean_donut', 1, 135);

INSERT INTO order_line (order_id, product_id, quantity, price)
VALUES ('order1', 'hover_cookies', 2, 199);

-- Order 2: Doc's order
INSERT INTO order_line (order_id, product_id, quantity, price)
VALUES ('order2', 'time_tart', 1, 220);

-- Order 3: Doc's second order
INSERT INTO order_line (order_id, product_id, quantity, price)
VALUES ('order3', 'hover_cookies', 500, 199);
