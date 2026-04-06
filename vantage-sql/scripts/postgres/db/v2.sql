-- Hill Valley Bakery Database - PostgreSQL Version
-- Translated from SQLite v2.sql with PostgreSQL adaptations:
-- - BOOLEAN is native (not 0/1)
-- - BIGINT instead of INTEGER for numeric IDs where needed
-- - SERIAL for auto-increment
-- - timestamp for datetime

CREATE TABLE IF NOT EXISTS bakery (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    profit_margin BIGINT NOT NULL CHECK (profit_margin >= 0 AND profit_margin <= 100)
);

CREATE TABLE IF NOT EXISTS client (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    contact_details TEXT NOT NULL,
    is_paying_client BOOLEAN NOT NULL DEFAULT false,
    balance DOUBLE PRECISION NOT NULL DEFAULT 0,
    bakery_id TEXT NOT NULL REFERENCES bakery(id)
);

CREATE TABLE IF NOT EXISTS product (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    calories BIGINT NOT NULL CHECK (calories >= 0),
    price BIGINT NOT NULL CHECK (price > 0),
    bakery_id TEXT NOT NULL REFERENCES bakery(id),
    is_deleted BOOLEAN NOT NULL DEFAULT false,
    inventory_stock BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS client_order (
    id TEXT PRIMARY KEY,
    bakery_id TEXT NOT NULL REFERENCES bakery(id),
    client_id TEXT NOT NULL REFERENCES client(id),
    is_deleted BOOLEAN NOT NULL DEFAULT false,
    created_at TEXT NOT NULL DEFAULT to_char(now(), 'YYYY-MM-DD"T"HH24:MI:SS')
);

CREATE TABLE IF NOT EXISTS order_line (
    id SERIAL PRIMARY KEY,
    order_id TEXT NOT NULL REFERENCES client_order(id),
    product_id TEXT NOT NULL REFERENCES product(id),
    quantity BIGINT NOT NULL CHECK (quantity > 0),
    price BIGINT NOT NULL CHECK (price > 0)
);

-- Data

INSERT INTO bakery (id, name, profit_margin)
VALUES ('hill_valley', 'Hill Valley Bakery', 15)
ON CONFLICT DO NOTHING;

INSERT INTO client (id, name, email, contact_details, is_paying_client, balance, bakery_id)
VALUES
    ('marty', 'Marty McFly', 'marty@gmail.com', '555-1955', true, 150.00, 'hill_valley'),
    ('doc', 'Doc Brown', 'doc@brown.com', '555-1885', true, 500.50, 'hill_valley'),
    ('biff', 'Biff Tannen', 'biff-3293@hotmail.com', '555-1955', false, -50.25, 'hill_valley')
ON CONFLICT DO NOTHING;

INSERT INTO product (id, name, calories, price, bakery_id, is_deleted, inventory_stock)
VALUES
    ('flux_cupcake', 'Flux Capacitor Cupcake', 300, 120, 'hill_valley', false, 50),
    ('delorean_donut', 'DeLorean Doughnut', 250, 135, 'hill_valley', false, 30),
    ('time_tart', 'Time Traveler Tart', 200, 220, 'hill_valley', false, 20),
    ('sea_pie', 'Enchantment Under the Sea Pie', 350, 299, 'hill_valley', false, 15),
    ('hover_cookies', 'Hoverboard Cookies', 150, 199, 'hill_valley', false, 40)
ON CONFLICT DO NOTHING;

INSERT INTO client_order (id, bakery_id, client_id, is_deleted, created_at)
VALUES
    ('order1', 'hill_valley', 'marty', false, to_char(now(), 'YYYY-MM-DD"T"HH24:MI:SS')),
    ('order2', 'hill_valley', 'doc', false, to_char(now(), 'YYYY-MM-DD"T"HH24:MI:SS')),
    ('order3', 'hill_valley', 'doc', false, to_char(now(), 'YYYY-MM-DD"T"HH24:MI:SS'))
ON CONFLICT DO NOTHING;

INSERT INTO order_line (order_id, product_id, quantity, price)
VALUES
    ('order1', 'flux_cupcake', 3, 120),
    ('order1', 'delorean_donut', 1, 135),
    ('order1', 'hover_cookies', 2, 199),
    ('order2', 'time_tart', 1, 220),
    ('order3', 'hover_cookies', 500, 199)
ON CONFLICT DO NOTHING;
