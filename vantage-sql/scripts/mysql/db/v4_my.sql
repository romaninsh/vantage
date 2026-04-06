-- =============================================================================
-- MySQL Query Builder Test Suite — Schema + Seed Data
-- Target: MySQL 8.0+
-- =============================================================================
--
-- This schema deliberately uses MySQL-specific types and features
-- that do NOT exist in PostgreSQL or SQLite:
--
--   Types exercised:
--     TINYINT, SMALLINT, MEDIUMINT, INT, BIGINT (+ UNSIGNED + AUTO_INCREMENT),
--     FLOAT, DOUBLE, DECIMAL, BIT,
--     YEAR, DATE, TIME, DATETIME, TIMESTAMP (with ON UPDATE),
--     CHAR, VARCHAR, TINYTEXT, TEXT, MEDIUMTEXT,
--     BINARY, VARBINARY, BLOB,
--     ENUM (inline), SET (inline), JSON, POINT, GEOMETRY
--
--   DDL features exercised:
--     AUTO_INCREMENT, UNSIGNED, ENGINE=InnoDB, CHARACTER SET, COLLATE,
--     ON UPDATE CURRENT_TIMESTAMP, GENERATED ALWAYS AS (VIRTUAL + STORED),
--     SPATIAL INDEX, FULLTEXT INDEX, invisible index,
--     composite PRIMARY KEY, FOREIGN KEY ON DELETE CASCADE / SET NULL
--
-- =============================================================================

SET NAMES utf8mb4;


-- -----------------------------------------------------------------------------
-- 1. departments — self-referential, MEDIUMINT, UNSIGNED
-- -----------------------------------------------------------------------------
CREATE TABLE departments (
    id          MEDIUMINT UNSIGNED NOT NULL AUTO_INCREMENT,
    name        VARCHAR(100) NOT NULL,
    budget      DECIMAL(12, 2) NOT NULL DEFAULT 0.00,
    parent_id   MEDIUMINT UNSIGNED DEFAULT NULL,
    created_at  TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (id),
    UNIQUE KEY uq_dept_name (name),
    CONSTRAINT fk_dept_parent FOREIGN KEY (parent_id) REFERENCES departments(id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

INSERT INTO departments (id, name, budget, parent_id) VALUES
    (1, 'Engineering',   500000.00, NULL),
    (2, 'Backend',       200000.00, 1),
    (3, 'Frontend',      150000.00, 1),
    (4, 'Sales',         300000.00, NULL),
    (5, 'Enterprise',    180000.00, 4),
    (6, 'SMB',           120000.00, 4),
    (7, 'Design',         80000.00, NULL),
    (8, 'UX Research',    40000.00, 7);


-- -----------------------------------------------------------------------------
-- 2. users — ENUM, SET, BIT, YEAR, UNSIGNED, ON UPDATE CURRENT_TIMESTAMP,
--            GENERATED COLUMNS (VIRTUAL + STORED)
-- -----------------------------------------------------------------------------
CREATE TABLE users (
    id              INT UNSIGNED NOT NULL AUTO_INCREMENT,
    name            VARCHAR(255) NOT NULL,
    email           VARCHAR(255) NOT NULL,
    role            ENUM('admin','editor','viewer') NOT NULL DEFAULT 'viewer',
    department_id   MEDIUMINT UNSIGNED DEFAULT NULL,
    salary          DECIMAL(10, 2) UNSIGNED NOT NULL DEFAULT 0.00,
    is_active       BIT(1) NOT NULL DEFAULT b'1',
    permissions     SET('read','write','delete','admin') NOT NULL DEFAULT 'read',
    hire_year       YEAR NOT NULL DEFAULT (YEAR(CURDATE())),
    created_at      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    display_name    VARCHAR(520) GENERATED ALWAYS AS (CONCAT(name, ' <', email, '>')) VIRTUAL,
    salary_band     VARCHAR(10) GENERATED ALWAYS AS (
        CASE
            WHEN salary >= 100000 THEN 'senior'
            WHEN salary >= 60000  THEN 'mid'
            WHEN salary >= 30000  THEN 'junior'
            ELSE 'intern'
        END
    ) STORED,
    PRIMARY KEY (id),
    UNIQUE KEY uq_user_email (email),
    CONSTRAINT fk_user_dept FOREIGN KEY (department_id) REFERENCES departments(id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

INSERT INTO users (id, name, email, role, department_id, salary, is_active, permissions, hire_year, created_at) VALUES
    (1,  'Alice Chen',      'alice@example.com',   'admin',  2, 120000.00, b'1', 'read,write,delete,admin', 2022, '2024-01-15 09:00:00'),
    (2,  'Bob Martinez',    'bob@example.com',     'editor', 2,  95000.00, b'1', 'read,write',              2022, '2024-02-20 10:30:00'),
    (3,  'Carol White',     'carol@example.com',   'viewer', 3,  88000.00, b'1', 'read',                    2023, '2024-03-10 08:15:00'),
    (4,  'Dan Brown',       'dan@example.com',     'editor', 3,  72000.00, b'1', 'read,write',              2023, '2024-04-01 14:00:00'),
    (5,  'Eve Johnson',     'eve@example.com',     'admin',  5, 110000.00, b'1', 'read,write,delete,admin', 2021, '2024-05-12 11:45:00'),
    (6,  'Frank Lee',       'frank@example.com',   'viewer', 5,  65000.00, b'1', 'read',                    2023, '2024-06-01 09:00:00'),
    (7,  'Grace Kim',       'grace@example.com',   'editor', 6,  78000.00, b'0', 'read,write',              2023, '2024-07-15 16:30:00'),
    (8,  'Hank Patel',      'hank@example.com',    'viewer', 1,  55000.00, b'1', 'read',                    2024, '2024-08-20 10:00:00'),
    (9,  'Iris Novak',      'iris@example.com',    'viewer', 7,  62000.00, b'1', 'read',                    2024, '2024-09-01 13:20:00'),
    (10, 'Jake Torres',     'jake@example.com',    'editor', 8,  70000.00, b'1', 'read,write',              2024, '2024-10-10 08:00:00'),
    (11, 'Karen Hill',      'karen@example.com',   'viewer', NULL, 25000.00, b'0', 'read',                  2025, '2025-01-05 09:00:00'),
    (12, 'Leo Russo',       'leo@example.com',     'admin',  4, 130000.00, b'1', 'read,write,delete,admin', 2021, '2025-02-14 11:00:00');


-- -----------------------------------------------------------------------------
-- 3. products — JSON, POINT (spatial), FULLTEXT INDEX, UNSIGNED FLOAT
-- -----------------------------------------------------------------------------
CREATE TABLE products (
    id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    name        VARCHAR(255) NOT NULL,
    category    VARCHAR(100) NOT NULL DEFAULT 'uncategorized',
    price       DECIMAL(10, 2) UNSIGNED NOT NULL DEFAULT 0.00,
    cost        FLOAT UNSIGNED NOT NULL DEFAULT 0.0,
    stock       SMALLINT UNSIGNED NOT NULL DEFAULT 0,
    tags        VARCHAR(500) NOT NULL DEFAULT '' COMMENT 'Comma-separated for FIND_IN_SET',
    metadata    JSON DEFAULT NULL,
    description TEXT,
    location    POINT SRID 0 DEFAULT NULL,
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (id),
    FULLTEXT INDEX ft_prod_desc (name, description),
    SPATIAL INDEX sp_prod_loc (location)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

INSERT INTO products (id, name, category, price, cost, stock, tags, metadata, description, location) VALUES
    (1,  'Widget Pro',      'electronics',    29.99,  15.0,  150, 'featured,bestseller',     '{"color":"black","weight_kg":0.3,"rating":4.7,"specs":{"voltage":5,"watts":10}}',  'Professional-grade widget with advanced features',                 ST_GeomFromText('POINT(51.95 -0.18)', 0)),
    (2,  'Widget Basic',    'electronics',    14.99,   8.0,  300, 'sale',                    '{"color":"white","weight_kg":0.2,"rating":4.2,"specs":{"voltage":5,"watts":5}}',   'Entry-level widget for basic use',                                 ST_GeomFromText('POINT(52.52 13.40)', 0)),
    (3,  'Gadget Pro Max',  'electronics',    99.99,  55.0,   50, 'featured,premium',        '{"color":"silver","weight_kg":0.8,"rating":4.9,"specs":{"voltage":12,"watts":25}}','Top-of-the-line gadget with pro features and maximum performance',  ST_GeomFromText('POINT(48.85 2.35)', 0)),
    (4,  'Desk Lamp',       'home',           45.00,  20.0,   80, 'new',                     '{"color":"brass","weight_kg":2.1,"rating":4.1}',                                   'Adjustable brass desk lamp with warm LED',                         NULL),
    (5,  'Ergo Chair',      'furniture',     350.00, 180.0,   20, 'featured,premium',        '{"color":"gray","weight_kg":15.0,"rating":4.8}',                                   'Ergonomic office chair with lumbar support',                       NULL),
    (6,  'USB-C Cable',     'electronics',     9.99,   3.0,  500, 'sale,bestseller',         '{"color":"black","weight_kg":0.05,"rating":4.0}',                                  'Durable braided USB-C cable, 2 meters',                            ST_GeomFromText('POINT(40.71 -74.00)', 0)),
    (7,  'Notebook A5',     'stationery',      5.50,   2.0, 1000, '',                        '{"color":"blue","weight_kg":0.15,"rating":3.8}',                                   'Lined A5 notebook, 200 pages',                                     NULL),
    (8,  'Pen Set',         'stationery',     12.00,   5.0,  400, 'new',                     '{"color":"multi","weight_kg":0.1,"rating":4.3}',                                   'Premium ballpoint pen set of 5',                                   NULL),
    (9,  'Monitor 27"',    'electronics',   450.00, 250.0,   15, 'premium',                 '{"color":"black","weight_kg":6.5,"rating":4.6,"specs":{"voltage":110,"watts":45}}','27-inch 4K IPS monitor with HDR support',                          ST_GeomFromText('POINT(35.68 139.69)', 0)),
    (10, 'Keyboard Mech',   'electronics',    79.99,  40.0,   60, 'new,featured',            '{"color":"white","weight_kg":0.9,"rating":4.5}',                                   'Mechanical keyboard with Cherry MX switches',                      ST_GeomFromText('POINT(37.77 -122.41)', 0)),
    (11, 'Mousepad XL',     'electronics',    19.99,   5.0,  200, 'sale',                    '{"color":"black","weight_kg":0.4,"rating":3.5}',                                   'Extended mousepad with stitched edges',                             NULL),
    (12, 'Standing Desk',   'furniture',     600.00, 300.0,   10, 'premium',                 '{"color":"walnut","weight_kg":35.0,"rating":4.9}',                                 'Electric standing desk with memory presets',                       NULL),
    (13, 'Clearance Item',  'uncategorized',   2.00,   1.0,    0, 'clearance',               '{"color":null,"weight_kg":0.01,"rating":2.0}',                                     NULL,                                                                NULL);


-- -----------------------------------------------------------------------------
-- 4. orders — BIGINT UNSIGNED, DATETIME, FK to INT UNSIGNED
-- -----------------------------------------------------------------------------
CREATE TABLE orders (
    id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    user_id     INT UNSIGNED NOT NULL,
    total       DECIMAL(10, 2) NOT NULL DEFAULT 0.00,
    status      ENUM('pending','shipped','completed','cancelled') NOT NULL DEFAULT 'pending',
    notes       TINYTEXT DEFAULT NULL,
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (id),
    INDEX idx_orders_user (user_id),
    INDEX idx_orders_status (status) INVISIBLE,
    CONSTRAINT fk_order_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
) ENGINE=InnoDB;

INSERT INTO orders (id, user_id, total, status, notes, created_at) VALUES
    (1,  1,   250.00, 'completed', 'Rush delivery',          '2025-01-10 10:00:00'),
    (2,  1,   125.50, 'completed', NULL,                     '2025-02-14 14:30:00'),
    (3,  1,    75.00, 'shipped',   'Gift wrap requested',    '2025-03-20 09:15:00'),
    (4,  2,   500.00, 'completed', NULL,                     '2025-01-22 11:00:00'),
    (5,  2,    60.00, 'cancelled', 'Customer changed mind',  '2025-02-05 16:45:00'),
    (6,  3,   310.00, 'completed', NULL,                     '2025-03-01 10:30:00'),
    (7,  3,    45.00, 'pending',   NULL,                     '2025-04-02 08:00:00'),
    (8,  5,  1200.00, 'completed', 'Corporate account',      '2025-01-30 12:00:00'),
    (9,  5,   800.00, 'shipped',   NULL,                     '2025-03-15 15:20:00'),
    (10, 6,    90.00, 'completed', NULL,                     '2025-02-28 09:00:00'),
    (11, 7,   150.00, 'cancelled', NULL,                     '2025-03-10 14:00:00'),
    (12, 10,  420.00, 'completed', 'Repeat customer',        '2025-04-01 10:00:00'),
    (13, 12,  350.00, 'shipped',   NULL,                     '2025-03-25 11:30:00'),
    (14, 12,  275.00, 'completed', NULL,                     '2025-04-03 09:00:00'),
    (15, 1,    50.00, 'pending',   'Hold for pickup',        '2025-04-05 08:00:00');


-- -----------------------------------------------------------------------------
-- 5. order_items — DOUBLE, GENERATED STORED, composite unique
-- -----------------------------------------------------------------------------
CREATE TABLE order_items (
    id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    order_id    BIGINT UNSIGNED NOT NULL,
    product_id  BIGINT UNSIGNED NOT NULL,
    quantity    SMALLINT UNSIGNED NOT NULL DEFAULT 1,
    unit_price  DOUBLE NOT NULL,
    line_total  DOUBLE GENERATED ALWAYS AS (quantity * unit_price) STORED,
    PRIMARY KEY (id),
    UNIQUE KEY uq_order_product (order_id, product_id),
    CONSTRAINT fk_oi_order   FOREIGN KEY (order_id)   REFERENCES orders(id)   ON DELETE CASCADE,
    CONSTRAINT fk_oi_product FOREIGN KEY (product_id) REFERENCES products(id) ON DELETE CASCADE
) ENGINE=InnoDB;

INSERT INTO order_items (order_id, product_id, quantity, unit_price) VALUES
    (1,  1,  2, 29.99), (1,  6,  5,  9.99),
    (2,  2,  3, 14.99), (2,  7, 10,  5.50),
    (3,  6,  2,  9.99), (3,  8,  1, 12.00),
    (4,  3,  1, 99.99), (4,  5,  1, 350.00),
    (5,  7,  5,  5.50), (5,  8,  2, 12.00),
    (6,  9,  1, 450.00),
    (7,  7,  3,  5.50),
    (8, 12,  1, 600.00), (8,  5,  1, 350.00),
    (9, 10,  2,  79.99), (9,  3,  1, 99.99),
    (10, 6,  5,  9.99),  (10,11,  1, 19.99),
    (11, 4,  2, 45.00),
    (12, 1,  3, 29.99),  (12,10,  1, 79.99),
    (13, 9,  1, 450.00),
    (14, 2,  5, 14.99),  (14, 8,  2, 12.00),
    (15, 7,  1,  5.50);


-- -----------------------------------------------------------------------------
-- 6. tags — simple lookup
-- -----------------------------------------------------------------------------
CREATE TABLE tags (
    id   INT UNSIGNED NOT NULL AUTO_INCREMENT,
    name VARCHAR(50) NOT NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_tag_name (name)
) ENGINE=InnoDB;

INSERT INTO tags (id, name) VALUES
    (1, 'electronics'), (2, 'sale'), (3, 'featured'),
    (4, 'new'), (5, 'clearance'), (6, 'premium'), (7, 'bestseller');


-- -----------------------------------------------------------------------------
-- 7. product_tags — junction table
-- -----------------------------------------------------------------------------
CREATE TABLE product_tags (
    product_id  BIGINT UNSIGNED NOT NULL,
    tag_id      INT UNSIGNED NOT NULL,
    PRIMARY KEY (product_id, tag_id),
    CONSTRAINT fk_pt_product FOREIGN KEY (product_id) REFERENCES products(id) ON DELETE CASCADE,
    CONSTRAINT fk_pt_tag     FOREIGN KEY (tag_id)      REFERENCES tags(id)     ON DELETE CASCADE
) ENGINE=InnoDB;

INSERT INTO product_tags (product_id, tag_id) VALUES
    (1,1),(1,3),(1,7),   (2,1),(2,2),
    (3,1),(3,3),(3,6),   (5,3),(5,6),
    (6,1),(6,2),(6,7),   (9,1),(9,6),
    (10,1),(10,4),       (11,1),(11,2),
    (12,3),(12,6),       (13,5);


-- -----------------------------------------------------------------------------
-- 8. audit_log — BINARY(16), MEDIUMTEXT, TIMESTAMP with fractional seconds
-- -----------------------------------------------------------------------------
CREATE TABLE audit_log (
    id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    table_name  VARCHAR(64) NOT NULL,
    row_id      BIGINT UNSIGNED NOT NULL,
    action      ENUM('INSERT','UPDATE','DELETE') NOT NULL,
    details     MEDIUMTEXT,
    session_id  BINARY(16) DEFAULT NULL COMMENT 'Raw UUID bytes',
    changed_at  TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    INDEX idx_audit_table_row (table_name, row_id)
) ENGINE=InnoDB;

INSERT INTO audit_log (id, table_name, row_id, action, details, session_id, changed_at) VALUES
    (1, 'users',    1,  'INSERT', 'Created admin user Alice',                     UNHEX(REPLACE('a0000001-0000-0000-0000-000000000001','-','')), '2024-01-15 09:00:00.123456'),
    (2, 'users',    2,  'INSERT', 'Created editor user Bob',                      UNHEX(REPLACE('a0000002-0000-0000-0000-000000000002','-','')), '2024-02-20 10:30:00.654321'),
    (3, 'orders',   1,  'INSERT', 'Order #1 placed by Alice',                     NULL,                                                          '2025-01-10 10:00:00.000001'),
    (4, 'orders',   1,  'UPDATE', 'Order #1 status changed to completed',         NULL,                                                          '2025-01-12 14:00:00.100000'),
    (5, 'products', 13, 'UPDATE', 'Clearance Item stock set to 0',                NULL,                                                          '2025-03-01 08:00:00.000000'),
    (6, 'users',    7,  'UPDATE', 'Grace Kim deactivated — is_active set to 0',   NULL,                                                          '2025-03-15 10:00:00.500000');


-- -----------------------------------------------------------------------------
-- 9. schedules — TIME type, BIT flags
-- -----------------------------------------------------------------------------
CREATE TABLE schedules (
    id          INT UNSIGNED NOT NULL AUTO_INCREMENT,
    user_id     INT UNSIGNED NOT NULL,
    day_of_week TINYINT UNSIGNED NOT NULL CHECK (day_of_week BETWEEN 0 AND 6),
    start_time  TIME NOT NULL,
    end_time    TIME NOT NULL,
    flags       BIT(8) NOT NULL DEFAULT b'00000000' COMMENT 'Bitmask: 1=remote,2=flexible,4=oncall',
    PRIMARY KEY (id),
    CONSTRAINT fk_sched_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
) ENGINE=InnoDB;

INSERT INTO schedules (user_id, day_of_week, start_time, end_time, flags) VALUES
    (1, 1, '09:00:00', '17:30:00', b'00000011'),  -- Mon, remote+flexible
    (1, 2, '09:00:00', '17:30:00', b'00000001'),  -- Tue, remote
    (1, 3, '09:00:00', '17:30:00', b'00000000'),  -- Wed, office
    (2, 1, '10:00:00', '18:00:00', b'00000001'),  -- Mon, remote
    (2, 2, '10:00:00', '18:00:00', b'00000001'),
    (5, 1, '08:00:00', '16:00:00', b'00000101'),  -- Mon, remote+oncall
    (5, 4, '08:00:00', '16:00:00', b'00000100'),  -- Thu, oncall
    (10, 1, '09:30:00', '17:00:00', b'00000010'), -- Mon, flexible
    (10, 5, '09:30:00', '13:00:00', b'00000010'); -- Fri, flexible (half day)
