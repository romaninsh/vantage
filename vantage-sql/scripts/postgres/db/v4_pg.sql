-- =============================================================================
-- PostgreSQL Query Builder Test Suite — Schema + Seed Data
-- Target: PostgreSQL 16+ (all features here work on 14+)
-- =============================================================================
--
-- This schema deliberately uses PostgreSQL-specific types and features
-- that do NOT exist in SQLite or MySQL, giving your query builder
-- extension a rich surface to cover:
--
--   Types exercised:
--     SERIAL/BIGSERIAL, BOOLEAN (true bool), UUID, TIMESTAMPTZ, INTERVAL,
--     NUMERIC, MONEY, TEXT[], INTEGER[], JSONB, BYTEA, INET, DATERANGE,
--     TSTZRANGE, POINT, ENUM (custom)
--
--   DDL features exercised:
--     CREATE TYPE (enum), GENERATED ALWAYS AS IDENTITY, GENERATED STORED,
--     ARRAY columns, JSONB with GIN index, EXCLUDE USING GIST (range),
--     UNIQUE, NOT NULL, DEFAULT with expressions, CHECK, FOREIGN KEY,
--     partial index, expression index
--
-- =============================================================================

-- Enum type for ticket priority
CREATE TYPE priority_level AS ENUM ('low', 'medium', 'high', 'critical');

-- Enum type for ticket status
CREATE TYPE ticket_status AS ENUM ('open', 'in_progress', 'review', 'closed');


-- -----------------------------------------------------------------------------
-- 1. departments — basic table with SERIAL
-- -----------------------------------------------------------------------------
CREATE TABLE departments (
    id          SERIAL PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    budget      NUMERIC(12, 2) NOT NULL DEFAULT 0,
    parent_id   INTEGER REFERENCES departments(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO departments (id, name, budget, parent_id) VALUES
    (1, 'Engineering',   500000.00,  NULL),
    (2, 'Backend',       200000.00,  1),
    (3, 'Frontend',      150000.00,  1),
    (4, 'Sales',         300000.00,  NULL),
    (5, 'Enterprise',    180000.00,  4),
    (6, 'SMB',           120000.00,  4),
    (7, 'Design',         80000.00,  NULL),
    (8, 'UX Research',    40000.00,  7);
SELECT setval('departments_id_seq', 8);


-- -----------------------------------------------------------------------------
-- 2. users — UUID PK, BOOLEAN, TEXT[], INET, TIMESTAMPTZ, INTERVAL
-- -----------------------------------------------------------------------------
CREATE TABLE users (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    email           TEXT NOT NULL UNIQUE,
    role            TEXT NOT NULL DEFAULT 'viewer' CHECK (role IN ('admin', 'editor', 'viewer')),
    department_id   INTEGER REFERENCES departments(id) ON DELETE SET NULL,
    salary          NUMERIC(10, 2) NOT NULL DEFAULT 0,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    skills          TEXT[] DEFAULT '{}',
    last_login_ip   INET,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    tenure          INTERVAL GENERATED ALWAYS AS (NOW() - created_at) STORED
);

INSERT INTO users (id, name, email, role, department_id, salary, is_active, skills, last_login_ip, created_at) VALUES
    ('a0000000-0000-0000-0000-000000000001', 'Alice Chen',    'alice@example.com',  'admin',  2, 120000, TRUE,  ARRAY['rust','postgresql','kubernetes'],   '192.168.1.10',   '2024-01-15 09:00:00+00'),
    ('a0000000-0000-0000-0000-000000000002', 'Bob Martinez',  'bob@example.com',    'editor', 2,  95000, TRUE,  ARRAY['python','django'],                  '10.0.0.42',      '2024-02-20 10:30:00+00'),
    ('a0000000-0000-0000-0000-000000000003', 'Carol White',   'carol@example.com',  'viewer', 3,  88000, TRUE,  ARRAY['typescript','react','css'],          '172.16.0.5',     '2024-03-10 08:15:00+00'),
    ('a0000000-0000-0000-0000-000000000004', 'Dan Brown',     'dan@example.com',    'editor', 3,  72000, TRUE,  ARRAY['javascript','vue'],                 '192.168.1.20',   '2024-04-01 14:00:00+00'),
    ('a0000000-0000-0000-0000-000000000005', 'Eve Johnson',   'eve@example.com',    'admin',  5, 110000, TRUE,  ARRAY['go','grpc','aws'],                  '10.0.0.100',     '2024-05-12 11:45:00+00'),
    ('a0000000-0000-0000-0000-000000000006', 'Frank Lee',     'frank@example.com',  'viewer', 5,  65000, TRUE,  ARRAY['excel','sql'],                      NULL,             '2024-06-01 09:00:00+00'),
    ('a0000000-0000-0000-0000-000000000007', 'Grace Kim',     'grace@example.com',  'editor', 6,  78000, FALSE, ARRAY['python','data-analysis'],           '192.168.1.30',   '2024-07-15 16:30:00+00'),
    ('a0000000-0000-0000-0000-000000000008', 'Hank Patel',    'hank@example.com',   'viewer', 1,  55000, TRUE,  ARRAY['c','linux'],                        '10.0.0.5',       '2024-08-20 10:00:00+00'),
    ('a0000000-0000-0000-0000-000000000009', 'Iris Novak',    'iris@example.com',   'viewer', 7,  62000, TRUE,  ARRAY['figma','sketch'],                   NULL,             '2024-09-01 13:20:00+00'),
    ('a0000000-0000-0000-0000-000000000010', 'Jake Torres',   'jake@example.com',   'editor', 8,  70000, TRUE,  ARRAY['figma','css','usability-testing'],  '172.16.0.10',    '2024-10-10 08:00:00+00'),
    ('a0000000-0000-0000-0000-000000000011', 'Karen Hill',    'karen@example.com',  'viewer', NULL, 25000, FALSE, '{}',                                    NULL,             '2025-01-05 09:00:00+00'),
    ('a0000000-0000-0000-0000-000000000012', 'Leo Russo',     'leo@example.com',    'admin',  4, 130000, TRUE,  ARRAY['leadership','strategy'],            '10.0.0.1',       '2025-02-14 11:00:00+00');


-- -----------------------------------------------------------------------------
-- 3. products — JSONB, MONEY, BYTEA, TEXT[], expression index
-- -----------------------------------------------------------------------------
CREATE TABLE products (
    id          BIGSERIAL PRIMARY KEY,
    name        TEXT NOT NULL,
    category    TEXT NOT NULL DEFAULT 'uncategorized',
    price       MONEY NOT NULL,
    cost        NUMERIC(10, 2) NOT NULL DEFAULT 0,
    tags        TEXT[] DEFAULT '{}',
    metadata    JSONB NOT NULL DEFAULT '{}',
    thumbnail   BYTEA,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_products_metadata ON products USING GIN (metadata);
CREATE INDEX idx_products_tags     ON products USING GIN (tags);
CREATE INDEX idx_products_category ON products (category) WHERE category != 'uncategorized';

INSERT INTO products (id, name, category, price, cost, tags, metadata) VALUES
    (1,  'Widget Pro',      'electronics', '$29.99',  15.00, ARRAY['featured','bestseller'], '{"color":"black","weight_kg":0.3,"rating":4.7,"specs":{"voltage":5,"watts":10}}'),
    (2,  'Widget Basic',    'electronics', '$14.99',   8.00, ARRAY['sale'],                  '{"color":"white","weight_kg":0.2,"rating":4.2,"specs":{"voltage":5,"watts":5}}'),
    (3,  'Gadget Pro Max',  'electronics', '$99.99',  55.00, ARRAY['featured','premium'],    '{"color":"silver","weight_kg":0.8,"rating":4.9,"specs":{"voltage":12,"watts":25}}'),
    (4,  'Desk Lamp',       'home',        '$45.00',  20.00, ARRAY['new'],                   '{"color":"brass","weight_kg":2.1,"rating":4.1}'),
    (5,  'Ergo Chair',      'furniture',  '$350.00', 180.00, ARRAY['featured','premium'],    '{"color":"gray","weight_kg":15.0,"rating":4.8}'),
    (6,  'USB-C Cable',     'electronics',  '$9.99',   3.00, ARRAY['sale','bestseller'],     '{"color":"black","weight_kg":0.05,"rating":4.0}'),
    (7,  'Notebook A5',     'stationery',   '$5.50',   2.00, '{}',                           '{"color":"blue","weight_kg":0.15,"rating":3.8}'),
    (8,  'Pen Set',         'stationery',  '$12.00',   5.00, ARRAY['new'],                   '{"color":"multi","weight_kg":0.1,"rating":4.3}'),
    (9,  'Monitor 27"',    'electronics', '$450.00', 250.00, ARRAY['premium'],               '{"color":"black","weight_kg":6.5,"rating":4.6,"specs":{"voltage":110,"watts":45}}'),
    (10, 'Keyboard Mech',   'electronics',  '$79.99',  40.00, ARRAY['new','featured'],       '{"color":"white","weight_kg":0.9,"rating":4.5}'),
    (11, 'Mousepad XL',     'electronics',  '$19.99',  5.00, ARRAY['sale'],                  '{"color":"black","weight_kg":0.4,"rating":3.5}'),
    (12, 'Standing Desk',   'furniture',   '$600.00', 300.00, ARRAY['premium'],               '{"color":"walnut","weight_kg":35.0,"rating":4.9}'),
    (13, 'Clearance Item',  'uncategorized','$2.00',   1.00, ARRAY['clearance'],             '{"color":null,"weight_kg":0.01,"rating":2.0}');
SELECT setval('products_id_seq', 13);


-- -----------------------------------------------------------------------------
-- 4. orders — TIMESTAMPTZ, FK to UUID column
-- -----------------------------------------------------------------------------
CREATE TABLE orders (
    id          BIGSERIAL PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    total       NUMERIC(10, 2) NOT NULL CHECK (total >= 0),
    status      TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','shipped','completed','cancelled')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO orders (id, user_id, total, status, created_at) VALUES
    (1,  'a0000000-0000-0000-0000-000000000001',  250.00, 'completed', '2025-01-10 10:00:00+00'),
    (2,  'a0000000-0000-0000-0000-000000000001',  125.50, 'completed', '2025-02-14 14:30:00+00'),
    (3,  'a0000000-0000-0000-0000-000000000001',   75.00, 'shipped',   '2025-03-20 09:15:00+00'),
    (4,  'a0000000-0000-0000-0000-000000000002',  500.00, 'completed', '2025-01-22 11:00:00+00'),
    (5,  'a0000000-0000-0000-0000-000000000002',   60.00, 'cancelled', '2025-02-05 16:45:00+00'),
    (6,  'a0000000-0000-0000-0000-000000000003',  310.00, 'completed', '2025-03-01 10:30:00+00'),
    (7,  'a0000000-0000-0000-0000-000000000003',   45.00, 'pending',   '2025-04-02 08:00:00+00'),
    (8,  'a0000000-0000-0000-0000-000000000005', 1200.00, 'completed', '2025-01-30 12:00:00+00'),
    (9,  'a0000000-0000-0000-0000-000000000005',  800.00, 'shipped',   '2025-03-15 15:20:00+00'),
    (10, 'a0000000-0000-0000-0000-000000000006',   90.00, 'completed', '2025-02-28 09:00:00+00'),
    (11, 'a0000000-0000-0000-0000-000000000007',  150.00, 'cancelled', '2025-03-10 14:00:00+00'),
    (12, 'a0000000-0000-0000-0000-000000000010',  420.00, 'completed', '2025-04-01 10:00:00+00'),
    (13, 'a0000000-0000-0000-0000-000000000012',  350.00, 'shipped',   '2025-03-25 11:30:00+00'),
    (14, 'a0000000-0000-0000-0000-000000000012',  275.00, 'completed', '2025-04-03 09:00:00+00'),
    (15, 'a0000000-0000-0000-0000-000000000001',   50.00, 'pending',   '2025-04-05 08:00:00+00');
SELECT setval('orders_id_seq', 15);


-- -----------------------------------------------------------------------------
-- 5. order_items — integer arrays not needed here, but GENERATED STORED
-- -----------------------------------------------------------------------------
CREATE TABLE order_items (
    id          BIGSERIAL PRIMARY KEY,
    order_id    BIGINT NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_id  BIGINT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    quantity    SMALLINT NOT NULL DEFAULT 1 CHECK (quantity > 0),
    unit_price  NUMERIC(10, 2) NOT NULL,
    line_total  NUMERIC(10, 2) GENERATED ALWAYS AS (quantity * unit_price) STORED,
    UNIQUE(order_id, product_id)
);

INSERT INTO order_items (order_id, product_id, quantity, unit_price) VALUES
    (1,  1,  2, 29.99),  (1,  6,  5,  9.99),
    (2,  2,  3, 14.99),  (2,  7, 10,  5.50),
    (3,  6,  2,  9.99),  (3,  8,  1, 12.00),
    (4,  3,  1, 99.99),  (4,  5,  1, 350.00),
    (5,  7,  5,  5.50),  (5,  8,  2, 12.00),
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
SELECT setval('order_items_id_seq', 25);


-- -----------------------------------------------------------------------------
-- 6. tickets — custom ENUM types, ARRAY of integers, UUID FK
-- -----------------------------------------------------------------------------
CREATE TABLE tickets (
    id          BIGSERIAL PRIMARY KEY,
    title       TEXT NOT NULL,
    body        TEXT,
    priority    priority_level NOT NULL DEFAULT 'medium',
    status      ticket_status NOT NULL DEFAULT 'open',
    assignee_id UUID REFERENCES users(id) ON DELETE SET NULL,
    watcher_ids UUID[] DEFAULT '{}',
    tags        TEXT[] DEFAULT '{}',
    metadata    JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO tickets (id, title, body, priority, status, assignee_id, watcher_ids, tags, metadata, created_at, updated_at) VALUES
    (1, 'Fix login timeout',        'Users report 30s timeout on auth',             'critical', 'in_progress', 'a0000000-0000-0000-0000-000000000001', ARRAY['a0000000-0000-0000-0000-000000000002'::UUID], ARRAY['auth','backend'],   '{"sprint":12,"points":5}',  '2025-03-01 09:00:00+00', '2025-03-15 14:00:00+00'),
    (2, 'Update dashboard charts',   'Replace D3 with Recharts',                    'medium',   'open',        'a0000000-0000-0000-0000-000000000003', '{}',                                                ARRAY['frontend','ui'],     '{"sprint":12,"points":3}',  '2025-03-05 10:00:00+00', '2025-03-05 10:00:00+00'),
    (3, 'Database migration plan',   'Schema v2 requires downtime window',          'high',     'review',      'a0000000-0000-0000-0000-000000000001', ARRAY['a0000000-0000-0000-0000-000000000005'::UUID,'a0000000-0000-0000-0000-000000000002'::UUID], ARRAY['backend','devops'], '{"sprint":13,"points":8}', '2025-03-10 08:30:00+00', '2025-04-01 11:00:00+00'),
    (4, 'Add CSV export',            'Export filtered table data to CSV',            'low',      'open',        'a0000000-0000-0000-0000-000000000004', '{}',                                                ARRAY['frontend','feature'],'{"sprint":13,"points":2}',  '2025-03-12 14:00:00+00', '2025-03-12 14:00:00+00'),
    (5, 'Onboarding flow redesign',  'New user onboarding needs UX pass',           'medium',   'in_progress', 'a0000000-0000-0000-0000-000000000009', ARRAY['a0000000-0000-0000-0000-000000000010'::UUID], ARRAY['design','ux'],       '{"sprint":13,"points":5}',  '2025-03-15 09:00:00+00', '2025-04-02 16:00:00+00'),
    (6, 'API rate limiting',         'Implement token bucket for /api/v2',           'high',     'closed',      'a0000000-0000-0000-0000-000000000002', ARRAY['a0000000-0000-0000-0000-000000000001'::UUID], ARRAY['backend','security'],'{"sprint":11,"points":5}',  '2025-02-01 10:00:00+00', '2025-02-20 17:00:00+00'),
    (7, 'Sales dashboard metrics',   'Add MRR and churn to executive view',         'medium',   'open',        'a0000000-0000-0000-0000-000000000005', '{}',                                                ARRAY['frontend','sales'],  '{"sprint":14,"points":3}',  '2025-04-01 08:00:00+00', '2025-04-01 08:00:00+00'),
    (8, 'Fix mobile nav overlap',    'Hamburger menu overlaps search on iOS',       'low',      'closed',      'a0000000-0000-0000-0000-000000000003', '{}',                                                ARRAY['frontend','bug'],    '{"sprint":11,"points":1}',  '2025-02-10 11:00:00+00', '2025-02-12 09:00:00+00');
SELECT setval('tickets_id_seq', 8);


-- -----------------------------------------------------------------------------
-- 7. bookings — DATERANGE with EXCLUDE constraint (non-overlapping)
-- -----------------------------------------------------------------------------
CREATE TABLE bookings (
    id          BIGSERIAL PRIMARY KEY,
    room        TEXT NOT NULL,
    user_id     UUID NOT NULL REFERENCES users(id),
    during      DATERANGE NOT NULL,
    notes       TEXT,
    EXCLUDE USING GIST (room WITH =, during WITH &&)
);

INSERT INTO bookings (id, room, user_id, during, notes) VALUES
    (1, 'Room A', 'a0000000-0000-0000-0000-000000000001', '[2025-04-07, 2025-04-08)', 'Sprint planning'),
    (2, 'Room B', 'a0000000-0000-0000-0000-000000000005', '[2025-04-07, 2025-04-08)', 'Sales sync'),
    (3, 'Room A', 'a0000000-0000-0000-0000-000000000002', '[2025-04-08, 2025-04-09)', 'Design review'),
    (4, 'Room A', 'a0000000-0000-0000-0000-000000000001', '[2025-04-09, 2025-04-11)', 'Offsite prep'),
    (5, 'Room B', 'a0000000-0000-0000-0000-000000000010', '[2025-04-09, 2025-04-10)', 'User interviews'),
    (6, 'Room C', 'a0000000-0000-0000-0000-000000000012', '[2025-04-07, 2025-04-14)', 'Executive workshop');
SELECT setval('bookings_id_seq', 6);


-- -----------------------------------------------------------------------------
-- 8. locations — POINT type
-- -----------------------------------------------------------------------------
CREATE TABLE locations (
    id          SERIAL PRIMARY KEY,
    name        TEXT NOT NULL,
    coords      POINT NOT NULL,
    address     TEXT
);

INSERT INTO locations (id, name, coords, address) VALUES
    (1, 'HQ Office',         POINT(59.437, 24.7536),   'Tallinn, Estonia'),
    (2, 'Satellite Office',  POINT(52.5200, 13.4050),  'Berlin, Germany'),
    (3, 'Coworking Space',   POINT(48.8566, 2.3522),   'Paris, France'),
    (4, 'Remote Hub',        POINT(40.7128, -74.0060),  'New York, USA');
SELECT setval('locations_id_seq', 4);
