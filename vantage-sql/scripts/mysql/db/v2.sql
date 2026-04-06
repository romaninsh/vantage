-- Hill Valley Bakery Database - MySQL Version

CREATE TABLE IF NOT EXISTS bakery (
    id VARCHAR(255) PRIMARY KEY,
    name TEXT NOT NULL,
    profit_margin BIGINT NOT NULL CHECK (profit_margin >= 0 AND profit_margin <= 100)
);

CREATE TABLE IF NOT EXISTS client (
    id VARCHAR(255) PRIMARY KEY,
    email VARCHAR(255) NOT NULL UNIQUE,
    name TEXT NOT NULL,
    contact_details TEXT NOT NULL,
    is_paying_client BOOLEAN NOT NULL DEFAULT false,
    balance DOUBLE NOT NULL DEFAULT 0,
    bakery_id VARCHAR(255) NOT NULL,
    FOREIGN KEY (bakery_id) REFERENCES bakery(id)
);

CREATE TABLE IF NOT EXISTS product (
    id VARCHAR(255) PRIMARY KEY,
    name TEXT NOT NULL,
    calories BIGINT NOT NULL CHECK (calories >= 0),
    price BIGINT NOT NULL CHECK (price > 0),
    bakery_id VARCHAR(255) NOT NULL,
    is_deleted BOOLEAN NOT NULL DEFAULT false,
    inventory_stock BIGINT NOT NULL DEFAULT 0,
    sticker TEXT,
    FOREIGN KEY (bakery_id) REFERENCES bakery(id)
);

CREATE TABLE IF NOT EXISTS client_order (
    id VARCHAR(255) PRIMARY KEY,
    bakery_id VARCHAR(255) NOT NULL,
    client_id VARCHAR(255) NOT NULL,
    is_deleted BOOLEAN NOT NULL DEFAULT false,
    created_at VARCHAR(255) NOT NULL DEFAULT (DATE_FORMAT(NOW(), '%Y-%m-%dT%H:%i:%s')),
    FOREIGN KEY (bakery_id) REFERENCES bakery(id),
    FOREIGN KEY (client_id) REFERENCES client(id)
);

CREATE TABLE IF NOT EXISTS order_line (
    id INT AUTO_INCREMENT PRIMARY KEY,
    order_id VARCHAR(255) NOT NULL,
    product_id VARCHAR(255) NOT NULL,
    quantity BIGINT NOT NULL CHECK (quantity > 0),
    price BIGINT NOT NULL CHECK (price > 0),
    FOREIGN KEY (order_id) REFERENCES client_order(id),
    FOREIGN KEY (product_id) REFERENCES product(id)
);

-- Data

INSERT IGNORE INTO bakery (id, name, profit_margin)
VALUES ('hill_valley', 'Hill Valley Bakery', 15);

INSERT IGNORE INTO client (id, name, email, contact_details, is_paying_client, balance, bakery_id)
VALUES
    ('marty', 'Marty McFly', 'marty@gmail.com', '555-1955', true, 150.00, 'hill_valley'),
    ('doc', 'Doc Brown', 'doc@brown.com', '555-1885', true, 500.50, 'hill_valley'),
    ('biff', 'Biff Tannen', 'biff-3293@hotmail.com', '555-1955', false, -50.25, 'hill_valley');

INSERT IGNORE INTO product (id, name, calories, price, bakery_id, is_deleted, inventory_stock, sticker)
VALUES
    ('flux_cupcake', 'Flux Capacitor Cupcake', 300, 120, 'hill_valley', false, 50, 'cat'),
    ('delorean_donut', 'DeLorean Doughnut', 250, 135, 'hill_valley', false, 30, 'dog'),
    ('time_tart', 'Time Traveler Tart', 200, 220, 'hill_valley', false, 20, NULL),
    ('sea_pie', 'Enchantment Under the Sea Pie', 350, 299, 'hill_valley', false, 15, 'pig'),
    ('hover_cookies', 'Hoverboard Cookies', 150, 199, 'hill_valley', false, 40, 'chicken');

INSERT IGNORE INTO client_order (id, bakery_id, client_id, is_deleted, created_at)
VALUES
    ('order1', 'hill_valley', 'marty', false, DATE_FORMAT(NOW(), '%Y-%m-%dT%H:%i:%s')),
    ('order2', 'hill_valley', 'doc', false, DATE_FORMAT(NOW(), '%Y-%m-%dT%H:%i:%s')),
    ('order3', 'hill_valley', 'doc', false, DATE_FORMAT(NOW(), '%Y-%m-%dT%H:%i:%s'));

INSERT IGNORE INTO order_line (order_id, product_id, quantity, price)
VALUES
    ('order1', 'flux_cupcake', 3, 120),
    ('order1', 'delorean_donut', 1, 135),
    ('order1', 'hover_cookies', 2, 199),
    ('order2', 'time_tart', 1, 220),
    ('order3', 'hover_cookies', 500, 199);
