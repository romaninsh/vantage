-- =============================================================================
-- SQLite Query Builder Test Suite — Schema + Seed Data
-- Target: SQLite 3.51.x
-- =============================================================================
--
-- This schema deliberately uses a wide range of declared type spellings
-- to exercise every type affinity path your builder must handle:
--
--   INTEGER affinity : INTEGER, INT, TINYINT, SMALLINT, MEDIUMINT, BIGINT
--   TEXT    affinity : TEXT, VARCHAR(N), CHAR(N), CLOB, NVARCHAR(N)
--   REAL    affinity : REAL, FLOAT, DOUBLE, DOUBLE PRECISION
--   NUMERIC affinity : NUMERIC, DECIMAL(P,S), BOOLEAN, DATE, DATETIME
--   BLOB    affinity : BLOB
--   ANY     (STRICT) : ANY
--
-- Also exercises:
--   PRIMARY KEY, AUTOINCREMENT, NOT NULL, UNIQUE, DEFAULT, CHECK,
--   FOREIGN KEY, ON DELETE CASCADE / SET NULL, COLLATE, GENERATED ALWAYS
--   (VIRTUAL + STORED), WITHOUT ROWID, STRICT, composite PRIMARY KEY
--
-- =============================================================================

PRAGMA foreign_keys = ON;

-- -----------------------------------------------------------------------------
-- 1. departments — self-referential hierarchy
--    Types: INTEGER, TEXT, REAL, INT (FK to self)
-- -----------------------------------------------------------------------------
CREATE TABLE departments (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,
    budget      REAL DEFAULT 0.0,
    parent_id   INT REFERENCES departments(id) ON DELETE SET NULL
);

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
-- 2. users — widest spread of integer + text type spellings
--    Types: INTEGER, VARCHAR(255), TEXT, TINYINT, REAL, BOOLEAN, DATETIME
--    Features: CHECK, DEFAULT, COLLATE, generated column (VIRTUAL)
-- -----------------------------------------------------------------------------
CREATE TABLE users (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            VARCHAR(255) NOT NULL COLLATE NOCASE,
    email           VARCHAR(255) NOT NULL UNIQUE,
    role            TEXT NOT NULL DEFAULT 'viewer' CHECK (role IN ('admin','editor','viewer')),
    department_id   INT REFERENCES departments(id) ON DELETE SET NULL,
    salary          REAL NOT NULL DEFAULT 0.0,
    is_active       BOOLEAN NOT NULL DEFAULT 1,
    created_at      DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S','now')),
    display_name    TEXT GENERATED ALWAYS AS (name || ' <' || email || '>') VIRTUAL
);

INSERT INTO users (id, name, email, role, department_id, salary, is_active, created_at) VALUES
    (1,  'Alice Chen',      'alice@example.com',    'admin',  2, 120000.0, 1, '2024-01-15T09:00:00'),
    (2,  'Bob Martinez',    'bob@example.com',      'editor', 2,  95000.0, 1, '2024-02-20T10:30:00'),
    (3,  'Carol White',     'carol@example.com',    'viewer', 3,  88000.0, 1, '2024-03-10T08:15:00'),
    (4,  'Dan Brown',       'dan@example.com',      'editor', 3,  72000.0, 1, '2024-04-01T14:00:00'),
    (5,  'Eve Johnson',     'eve@example.com',      'admin',  5, 110000.0, 1, '2024-05-12T11:45:00'),
    (6,  'Frank Lee',       'frank@example.com',    'viewer', 5,  65000.0, 1, '2024-06-01T09:00:00'),
    (7,  'Grace Kim',       'grace@example.com',    'editor', 6,  78000.0, 0, '2024-07-15T16:30:00'),
    (8,  'Hank Patel',      'hank@example.com',     'viewer', 1,  55000.0, 1, '2024-08-20T10:00:00'),
    (9,  'Iris Novak',      'iris@example.com',     'viewer', 7,  62000.0, 1, '2024-09-01T13:20:00'),
    (10, 'Jake Torres',     'jake@example.com',     'editor', 8,  70000.0, 1, '2024-10-10T08:00:00'),
    (11, 'Karen Hill',      'karen@example.com',    'viewer', NULL, 25000.0, 0, '2025-01-05T09:00:00'),
    (12, 'Leo Russo',       'leo@example.com',      'admin',  4, 130000.0, 1, '2025-02-14T11:00:00');

-- -----------------------------------------------------------------------------
-- 3. orders — temporal types, status text
--    Types: INTEGER, INT (FK), NUMERIC (for total), TEXT, DATETIME
--    Features: CHECK on status, DEFAULT datetime
-- -----------------------------------------------------------------------------
CREATE TABLE orders (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    total       NUMERIC(10,2) NOT NULL CHECK (total >= 0),
    status      TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','shipped','completed','cancelled')),
    created_at  DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S','now'))
);

INSERT INTO orders (id, user_id, total, status, created_at) VALUES
    (1,  1,  250.00,  'completed',  '2025-01-10T10:00:00'),
    (2,  1,  125.50,  'completed',  '2025-02-14T14:30:00'),
    (3,  1,   75.00,  'shipped',    '2025-03-20T09:15:00'),
    (4,  2,  500.00,  'completed',  '2025-01-22T11:00:00'),
    (5,  2,   60.00,  'cancelled',  '2025-02-05T16:45:00'),
    (6,  3,  310.00,  'completed',  '2025-03-01T10:30:00'),
    (7,  3,   45.00,  'pending',    '2025-04-02T08:00:00'),
    (8,  5, 1200.00,  'completed',  '2025-01-30T12:00:00'),
    (9,  5,  800.00,  'shipped',    '2025-03-15T15:20:00'),
    (10, 6,   90.00,  'completed',  '2025-02-28T09:00:00'),
    (11, 7,  150.00,  'cancelled',  '2025-03-10T14:00:00'),
    (12, 10, 420.00,  'completed',  '2025-04-01T10:00:00'),
    (13, 12, 350.00,  'shipped',    '2025-03-25T11:30:00'),
    (14, 12, 275.00,  'completed',  '2025-04-03T09:00:00'),
    (15, 1,   50.00,  'pending',    '2025-04-05T08:00:00');

-- -----------------------------------------------------------------------------
-- 4. products — JSON metadata, BLOB, FLOAT, DECIMAL
--    Types: INTEGER, TEXT, FLOAT, DECIMAL(10,2), SMALLINT, BLOB
--    Features: JSON stored in TEXT column, CHECK
-- -----------------------------------------------------------------------------
CREATE TABLE products (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    category    TEXT NOT NULL DEFAULT 'uncategorized',
    price       DECIMAL(10,2) NOT NULL CHECK (price >= 0),
    stock       SMALLINT NOT NULL DEFAULT 0,
    metadata    TEXT,           -- stores JSON: {"color":"red","weight_kg":1.5,"rating":4.5,"in_stock":true}
    thumbnail   BLOB,
    created_at  DATE NOT NULL DEFAULT (date('now'))
);

INSERT INTO products (id, name, category, price, stock, metadata, thumbnail, created_at) VALUES
    (1,  'Widget Pro',          'electronics',    29.99,   150, '{"color":"black","weight_kg":0.3,"rating":4.7,"in_stock":1}',   NULL, '2025-01-01'),
    (2,  'Widget Basic',        'electronics',    14.99,   300, '{"color":"white","weight_kg":0.2,"rating":4.2,"in_stock":1}',   NULL, '2025-01-01'),
    (3,  'Gadget Pro Max',      'electronics',    99.99,    50, '{"color":"silver","weight_kg":0.8,"rating":4.9,"in_stock":1}',  NULL, '2025-01-15'),
    (4,  'Desk Lamp',           'home',           45.00,    80, '{"color":"brass","weight_kg":2.1,"rating":4.1,"in_stock":1}',   NULL, '2025-02-01'),
    (5,  'Ergonomic Chair',     'furniture',     350.00,    20, '{"color":"gray","weight_kg":15.0,"rating":4.8,"in_stock":1}',   NULL, '2025-02-10'),
    (6,  'USB-C Cable',         'electronics',     9.99,   500, '{"color":"black","weight_kg":0.05,"rating":4.0,"in_stock":1}',  NULL, '2025-02-15'),
    (7,  'Notebook A5',         'stationery',      5.50,  1000, '{"color":"blue","weight_kg":0.15,"rating":3.8,"in_stock":1}',   NULL, '2025-03-01'),
    (8,  'Pen Set',             'stationery',     12.00,   400, '{"color":"multi","weight_kg":0.1,"rating":4.3,"in_stock":1}',   NULL, '2025-03-01'),
    (9,  'Monitor 27"',        'electronics',   450.00,    15, '{"color":"black","weight_kg":6.5,"rating":4.6,"in_stock":1}',   NULL, '2025-03-10'),
    (10, 'Keyboard Mech',       'electronics',    79.99,    60, '{"color":"white","weight_kg":0.9,"rating":4.5,"in_stock":1}',   NULL, '2025-03-15'),
    (11, 'Mousepad XL',         'electronics',    19.99,   200, '{"color":"black","weight_kg":0.4,"rating":3.5,"in_stock":1}',   NULL, '2025-03-20'),
    (12, 'Standing Desk',       'furniture',     600.00,    10, '{"color":"walnut","weight_kg":35.0,"rating":4.9,"in_stock":1}', NULL, '2025-03-25'),
    (13, 'Clearance Item',      'uncategorized',   2.00,     0, '{"color":null,"weight_kg":0.01,"rating":2.0,"in_stock":0}',    NULL, '2024-06-01');

-- -----------------------------------------------------------------------------
-- 5. order_items — junction between orders and products
--    Types: INTEGER, INT (FKs), SMALLINT, DOUBLE PRECISION
--    Features: composite PK via UNIQUE, generated column (STORED)
-- -----------------------------------------------------------------------------
CREATE TABLE order_items (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id        INT NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_id      INT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    quantity        SMALLINT NOT NULL DEFAULT 1 CHECK (quantity > 0),
    unit_price      DOUBLE PRECISION NOT NULL,
    line_total      REAL GENERATED ALWAYS AS (quantity * unit_price) STORED,
    UNIQUE(order_id, product_id)
);

INSERT INTO order_items (order_id, product_id, quantity, unit_price) VALUES
    (1,  1,  2, 29.99),
    (1,  6,  5,  9.99),
    (2,  2,  3, 14.99),
    (2,  7,  10, 5.50),
    (3,  6,  2,  9.99),
    (3,  8,  1, 12.00),
    (4,  3,  1, 99.99),
    (4,  5,  1, 350.00),
    (5,  7,  5,  5.50),
    (5,  8,  2, 12.00),
    (6,  9,  1, 450.00),
    (7,  7,  3,  5.50),
    (8, 12,  1, 600.00),
    (8,  5,  1, 350.00),
    (9, 10,  2,  79.99),
    (9,  3,  1, 99.99),
    (10, 6,  5,  9.99),
    (10, 11, 1, 19.99),
    (11, 4,  2, 45.00),
    (12, 1,  3, 29.99),
    (12, 10, 1, 79.99),
    (13, 9,  1, 450.00),
    (14, 2,  5, 14.99),
    (14, 8,  2, 12.00),
    (15, 7,  1,  5.50);

-- -----------------------------------------------------------------------------
-- 6. tags — simple lookup
--    Types: INTEGER, TEXT
--    Features: UNIQUE
-- -----------------------------------------------------------------------------
CREATE TABLE tags (
    id   INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
);

INSERT INTO tags (id, name) VALUES
    (1, 'electronics'),
    (2, 'sale'),
    (3, 'featured'),
    (4, 'new'),
    (5, 'clearance'),
    (6, 'premium'),
    (7, 'bestseller');

-- -----------------------------------------------------------------------------
-- 7. product_tags — many-to-many junction (WITHOUT ROWID)
--    Types: INT, INT
--    Features: composite PRIMARY KEY, WITHOUT ROWID, FOREIGN KEYs
-- -----------------------------------------------------------------------------
CREATE TABLE product_tags (
    product_id  INT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    tag_id      INT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (product_id, tag_id)
) WITHOUT ROWID;

INSERT INTO product_tags (product_id, tag_id) VALUES
    (1,  1), (1,  3), (1, 7),   -- Widget Pro: electronics, featured, bestseller
    (2,  1), (2,  2),            -- Widget Basic: electronics, sale
    (3,  1), (3,  3), (3, 6),   -- Gadget Pro Max: electronics, featured, premium
    (5,  3), (5,  6),            -- Ergonomic Chair: featured, premium
    (6,  1), (6,  2), (6, 7),   -- USB-C Cable: electronics, sale, bestseller
    (9,  1), (9,  6),            -- Monitor: electronics, premium
    (10, 1), (10, 4),            -- Keyboard: electronics, new
    (11, 1), (11, 2),            -- Mousepad: electronics, sale
    (12, 3), (12, 6),            -- Standing Desk: featured, premium
    (13, 5);                     -- Clearance Item: clearance

-- -----------------------------------------------------------------------------
-- 8. audit_log — uses BIGINT, NVARCHAR, CHAR, CLOB
--    Types: BIGINT, NVARCHAR(100), CHAR(6), CLOB, DATETIME
--    Exercises rare type-spelling affinities
-- -----------------------------------------------------------------------------
CREATE TABLE audit_log (
    id          BIGINT PRIMARY KEY,
    table_name  NVARCHAR(100) NOT NULL,
    row_id      INT NOT NULL,
    action      CHAR(6) NOT NULL CHECK (action IN ('INSERT','UPDATE','DELETE')),
    details     CLOB,
    changed_at  DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S','now'))
);

INSERT INTO audit_log (id, table_name, row_id, action, details, changed_at) VALUES
    (1, 'users',    1,  'INSERT', 'Created admin user Alice',                     '2024-01-15T09:00:01'),
    (2, 'users',    2,  'INSERT', 'Created editor user Bob',                      '2024-02-20T10:30:01'),
    (3, 'orders',   1,  'INSERT', 'Order #1 placed',                              '2025-01-10T10:00:01'),
    (4, 'orders',   1,  'UPDATE', 'Order #1 status changed to completed',         '2025-01-12T14:00:00'),
    (5, 'products', 13, 'UPDATE', 'Clearance Item stock set to 0',                '2025-03-01T08:00:00'),
    (6, 'users',    7,  'UPDATE', 'Grace Kim deactivated',                        '2025-03-15T10:00:00'),
    (7, 'orders',   5,  'UPDATE', 'Order #5 cancelled',                           '2025-02-05T17:00:00'),
    (8, 'products', 1,  'UPDATE', 'Widget Pro price updated from 34.99 to 29.99', '2025-02-01T12:00:00');

-- -----------------------------------------------------------------------------
-- 9. settings — STRICT table with ANY column
--    Types (STRICT): INT, TEXT, ANY, BLOB
--    Features: STRICT table-option, ANY column
-- -----------------------------------------------------------------------------
CREATE TABLE settings (
    key     TEXT PRIMARY KEY,
    value   ANY,
    raw     BLOB
) STRICT;

INSERT INTO settings (key, value, raw) VALUES
    ('theme',           'dark',      NULL),
    ('max_retries',     5,           NULL),
    ('pi_approx',       3.14159,     NULL),
    ('banner_image',    NULL,        x'89504E470D0A1A0A'),   -- PNG header bytes
    ('feature_flags',   '{"beta":true,"v2":false}', NULL);

-- -----------------------------------------------------------------------------
-- 10. metrics — MEDIUMINT, FLOAT, TINYINT, generated columns
--     Types: INTEGER, MEDIUMINT, FLOAT, TINYINT, TEXT, DATE
--     Features: generated column (VIRTUAL + STORED), CHECK
-- -----------------------------------------------------------------------------
CREATE TABLE metrics (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INT NOT NULL REFERENCES users(id),
    page_views  MEDIUMINT NOT NULL DEFAULT 0,
    bounce_rate FLOAT NOT NULL DEFAULT 0.0 CHECK (bounce_rate >= 0.0 AND bounce_rate <= 1.0),
    is_bot      TINYINT NOT NULL DEFAULT 0,
    recorded_on DATE NOT NULL DEFAULT (date('now')),
    score       REAL GENERATED ALWAYS AS (page_views * (1.0 - bounce_rate)) VIRTUAL,
    label       TEXT GENERATED ALWAYS AS (
        CASE
            WHEN page_views >= 1000 THEN 'high'
            WHEN page_views >= 100  THEN 'medium'
            ELSE 'low'
        END
    ) STORED
);

INSERT INTO metrics (user_id, page_views, bounce_rate, is_bot, recorded_on) VALUES
    (1,  2500, 0.15, 0, '2025-04-01'),
    (1,  1800, 0.20, 0, '2025-04-02'),
    (2,   950, 0.45, 0, '2025-04-01'),
    (2,  1100, 0.40, 0, '2025-04-02'),
    (3,   300, 0.60, 0, '2025-04-01'),
    (5,  3000, 0.10, 0, '2025-04-01'),
    (5,    50, 0.90, 1, '2025-04-02'),
    (6,   200, 0.55, 0, '2025-04-01'),
    (10,  800, 0.30, 0, '2025-04-01'),
    (12, 4000, 0.05, 0, '2025-04-01');
