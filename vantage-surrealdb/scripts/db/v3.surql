-- Vantage v3 test database — expanded bakery model
-- 2 bakeries, departments, employees, suppliers, richer orders

-- =====================================================
-- SCHEMA
-- =====================================================

DEFINE TABLE bakery SCHEMALESS;
DEFINE FIELD name ON bakery TYPE string;
DEFINE FIELD profit_margin ON bakery TYPE int;

DEFINE TABLE department SCHEMALESS;
DEFINE FIELD name ON department TYPE string;
DEFINE FIELD budget ON department TYPE decimal;
DEFINE FIELD parent ON department TYPE option<record<department>>;
DEFINE FIELD bakery ON department TYPE record<bakery>;

DEFINE TABLE employee SCHEMALESS;
DEFINE FIELD name ON employee TYPE string;
DEFINE FIELD email ON employee TYPE string;
DEFINE FIELD salary ON employee TYPE decimal;
DEFINE FIELD role ON employee TYPE string;
DEFINE FIELD hire_date ON employee TYPE datetime;
DEFINE FIELD performance_rating ON employee TYPE int;
DEFINE FIELD department ON employee TYPE record<department>;
DEFINE FIELD bakery ON employee TYPE record<bakery>;
DEFINE FIELD manager ON employee TYPE option<record<employee>>;

DEFINE TABLE client SCHEMALESS;
DEFINE FIELD name ON client TYPE string;
DEFINE FIELD email ON client TYPE string;
DEFINE FIELD is_paying_client ON client TYPE bool DEFAULT false;
DEFINE FIELD balance ON client TYPE decimal DEFAULT 0;
DEFINE FIELD bakery ON client TYPE record<bakery>;

DEFINE TABLE product SCHEMALESS;
DEFINE FIELD name ON product TYPE string;
DEFINE FIELD calories ON product TYPE int;
DEFINE FIELD price ON product TYPE int;
DEFINE FIELD category ON product TYPE string;
DEFINE FIELD tags ON product TYPE array<string>;
DEFINE FIELD metadata ON product TYPE object;
DEFINE FIELD bakery ON product TYPE record<bakery>;
DEFINE FIELD inventory ON product TYPE object;
DEFINE FIELD inventory.stock ON product TYPE int DEFAULT 0;

DEFINE TABLE order SCHEMALESS;
DEFINE FIELD client ON order TYPE record<client>;
DEFINE FIELD bakery ON order TYPE record<bakery>;
DEFINE FIELD status ON order TYPE string;
DEFINE FIELD created_at ON order TYPE datetime;
DEFINE FIELD lines ON order TYPE array<object>;
DEFINE FIELD lines.*.product ON order TYPE record<product>;
DEFINE FIELD lines.*.quantity ON order TYPE int;
DEFINE FIELD lines.*.price ON order TYPE int;
DEFINE FIELD paid_at ON order TYPE option<datetime>;
DEFINE FIELD total ON order TYPE decimal;

DEFINE TABLE supplier SCHEMALESS;
DEFINE FIELD name ON supplier TYPE string;
DEFINE FIELD rating ON supplier TYPE int;
DEFINE FIELD free_delivery ON supplier TYPE bool;

DEFINE TABLE supply SCHEMALESS;
DEFINE FIELD supplier ON supply TYPE record<supplier>;
DEFINE FIELD product ON supply TYPE record<product>;
DEFINE FIELD unit_cost ON supply TYPE decimal;
DEFINE FIELD lead_time_days ON supply TYPE int;

-- Full-text search index on product name
DEFINE ANALYZER product_analyzer TOKENIZERS blank,class FILTERS lowercase;
DEFINE INDEX product_search ON product FIELDS name FULLTEXT ANALYZER product_analyzer BM25 HIGHLIGHTS;

-- =====================================================
-- SEED DATA
-- =====================================================

-- Bakeries
CREATE bakery:hill_valley SET name = 'Hill Valley Bakery', profit_margin = 15;
CREATE bakery:mr_cake SET name = 'Mr Cake Bakery', profit_margin = 20;

-- Departments (Hill Valley)
CREATE department:hv_eng SET name = 'Engineering', budget = 500000, bakery = bakery:hill_valley;
CREATE department:hv_frontend SET name = 'Frontend', budget = 200000, parent = department:hv_eng, bakery = bakery:hill_valley;
CREATE department:hv_backend SET name = 'Backend', budget = 250000, parent = department:hv_eng, bakery = bakery:hill_valley;
CREATE department:hv_sales SET name = 'Sales', budget = 300000, bakery = bakery:hill_valley;
CREATE department:hv_marketing SET name = 'Marketing', budget = 150000, parent = department:hv_sales, bakery = bakery:hill_valley;

-- Departments (Mr Cake)
CREATE department:mc_prod SET name = 'Production', budget = 400000, bakery = bakery:mr_cake;
CREATE department:mc_qc SET name = 'Quality Control', budget = 100000, parent = department:mc_prod, bakery = bakery:mr_cake;

-- Employees (Hill Valley)
CREATE employee:alice SET name = 'Alice Chen', email = 'alice@hv.com', salary = 120000,
    role = 'admin', hire_date = d'2020-03-15', performance_rating = 5,
    department = department:hv_backend, bakery = bakery:hill_valley;
CREATE employee:bob SET name = 'Bob Martinez', email = 'bob@hv.com', salary = 95000,
    role = 'dev', hire_date = d'2021-06-01', performance_rating = 4,
    department = department:hv_backend, bakery = bakery:hill_valley, manager = employee:alice;
CREATE employee:carol SET name = 'Carol Wu', email = 'carol@hv.com', salary = 85000,
    role = 'dev', hire_date = d'2022-01-10', performance_rating = 3,
    department = department:hv_frontend, bakery = bakery:hill_valley, manager = employee:alice;
CREATE employee:dave SET name = 'Dave Kim', email = 'dave@hv.com', salary = 110000,
    role = 'admin', hire_date = d'2019-09-20', performance_rating = 5,
    department = department:hv_sales, bakery = bakery:hill_valley;
CREATE employee:eve SET name = 'Eve Johnson', email = 'eve@hv.com', salary = 75000,
    role = 'sales', hire_date = d'2023-04-05', performance_rating = 4,
    department = department:hv_marketing, bakery = bakery:hill_valley, manager = employee:dave;
CREATE employee:frank SET name = 'Frank Lee', email = 'frank@hv.com', salary = 65000,
    role = 'sales', hire_date = d'2023-08-12', performance_rating = 2,
    department = department:hv_marketing, bakery = bakery:hill_valley, manager = employee:dave;

-- Employees (Mr Cake)
CREATE employee:grace SET name = 'Grace Park', email = 'grace@mc.com', salary = 130000,
    role = 'admin', hire_date = d'2018-02-01', performance_rating = 5,
    department = department:mc_prod, bakery = bakery:mr_cake;
CREATE employee:hank SET name = 'Hank Patel', email = 'hank@mc.com', salary = 90000,
    role = 'dev', hire_date = d'2020-11-15', performance_rating = 4,
    department = department:mc_qc, bakery = bakery:mr_cake, manager = employee:grace;
CREATE employee:iris SET name = 'Iris Novak', email = 'iris@mc.com', salary = 55000,
    role = 'intern', hire_date = d'2024-06-01', performance_rating = 3,
    department = department:mc_qc, bakery = bakery:mr_cake, manager = employee:hank;

-- Clients
CREATE client:marty SET name = 'Marty McFly', email = 'marty@gmail.com',
    is_paying_client = true, balance = 150.00, bakery = bakery:hill_valley;
CREATE client:doc SET name = 'Doc Brown', email = 'doc@brown.com',
    is_paying_client = true, balance = 500.50, bakery = bakery:hill_valley;
CREATE client:biff SET name = 'Biff Tannen', email = 'biff@hotmail.com',
    is_paying_client = false, balance = -50.25, bakery = bakery:hill_valley;
CREATE client:clara SET name = 'Clara Clayton', email = 'clara@brown.com',
    is_paying_client = true, balance = 320.00, bakery = bakery:mr_cake;
CREATE client:lorraine SET name = 'Lorraine Baines', email = 'lorraine@gmail.com',
    is_paying_client = true, balance = 200.00, bakery = bakery:mr_cake;
CREATE client:george SET name = 'George McFly', email = 'george@gmail.com',
    is_paying_client = false, balance = 10.00, bakery = bakery:hill_valley;

-- Products (Hill Valley)
CREATE product:flux_cupcake SET name = 'Flux Capacitor Cupcake', calories = 300, price = 120,
    category = 'pastry', tags = ['sweet', 'chocolate', 'signature'],
    metadata = { color: 'brown', weight_kg: 0.15, rating: 4.5, in_stock: true },
    bakery = bakery:hill_valley, inventory = { stock: 50 };
CREATE product:delorean_donut SET name = 'DeLorean Doughnut', calories = 250, price = 135,
    category = 'pastry', tags = ['sweet', 'glazed', 'bestseller'],
    metadata = { color: 'golden', weight_kg: 0.12, rating: 4.8, in_stock: true },
    bakery = bakery:hill_valley, inventory = { stock: 30 };
CREATE product:time_tart SET name = 'Time Traveler Tart', calories = 200, price = 220,
    category = 'pastry', tags = ['sweet', 'fruit', 'premium'],
    metadata = { color: 'red', weight_kg: 0.18, rating: 4.9, in_stock: true },
    bakery = bakery:hill_valley, inventory = { stock: 20 };
CREATE product:sea_pie SET name = 'Enchantment Under the Sea Pie', calories = 350, price = 299,
    category = 'savory', tags = ['savory', 'meat', 'signature'],
    metadata = { color: 'brown', weight_kg: 0.5, rating: 4.2, in_stock: true },
    bakery = bakery:hill_valley, inventory = { stock: 15 };
CREATE product:hover_cookies SET name = 'Hoverboard Cookies', calories = 150, price = 199,
    category = 'pastry', tags = ['sweet', 'cookies', 'bestseller'],
    metadata = { color: 'tan', weight_kg: 0.08, rating: 4.6, in_stock: true },
    bakery = bakery:hill_valley, inventory = { stock: 40 };

-- Products (Mr Cake)
CREATE product:mc_bread SET name = 'Artisan Sourdough Bread', calories = 180, price = 85,
    category = 'bread', tags = ['bread', 'sourdough', 'daily'],
    metadata = { color: 'brown', weight_kg: 0.8, rating: 4.7, in_stock: true },
    bakery = bakery:mr_cake, inventory = { stock: 100 };
CREATE product:mc_croissant SET name = 'Butter Croissant', calories = 280, price = 95,
    category = 'pastry', tags = ['sweet', 'breakfast', 'bestseller'],
    metadata = { color: 'golden', weight_kg: 0.1, rating: 4.4, in_stock: true },
    bakery = bakery:mr_cake, inventory = { stock: 60 };
CREATE product:mc_cake SET name = 'Triple Chocolate Cake', calories = 450, price = 350,
    category = 'cake', tags = ['sweet', 'chocolate', 'premium', 'signature'],
    metadata = { color: 'dark brown', weight_kg: 1.2, rating: 4.9, in_stock: true },
    bakery = bakery:mr_cake, inventory = { stock: 10 };
CREATE product:mc_sandwich SET name = 'Club Sandwich', calories = 400, price = 180,
    category = 'savory', tags = ['savory', 'lunch', 'daily'],
    metadata = { color: 'mixed', weight_kg: 0.35, rating: 4.1, in_stock: true },
    bakery = bakery:mr_cake, inventory = { stock: 25 };

-- Suppliers
CREATE supplier:acme_flour SET name = 'Acme Flour Co', rating = 5, free_delivery = true;
CREATE supplier:valley_dairy SET name = 'Valley Dairy Supply', rating = 4, free_delivery = false;
CREATE supplier:sugar_works SET name = 'Sugar Works', rating = 3, free_delivery = true;

-- Orders (Hill Valley)
CREATE order:order1 SET client = client:marty, bakery = bakery:hill_valley,
    status = 'placed', created_at = d'2025-01-15T10:30:00Z', total = 853.00,
    lines = [
        { product: product:flux_cupcake, quantity: 3, price: 120 },
        { product: product:delorean_donut, quantity: 1, price: 135 },
        { product: product:hover_cookies, quantity: 2, price: 199 }
    ];

CREATE order:order2 SET client = client:doc, bakery = bakery:hill_valley,
    status = 'completed', created_at = d'2025-01-20T14:00:00Z', total = 220.00,
    paid_at = d'2025-01-20T14:05:00Z',
    lines = [
        { product: product:time_tart, quantity: 1, price: 220 }
    ];

CREATE order:order3 SET client = client:doc, bakery = bakery:hill_valley,
    status = 'delivered', created_at = d'2025-02-10T09:00:00Z', total = 99500.00,
    paid_at = d'2025-02-10T09:10:00Z',
    lines = [
        { product: product:hover_cookies, quantity: 500, price: 199 }
    ];

CREATE order:order4 SET client = client:marty, bakery = bakery:hill_valley,
    status = 'cancelled', created_at = d'2025-02-15T11:00:00Z', total = 419.00,
    cancellation_reason = 'Changed mind',
    lines = [
        { product: product:sea_pie, quantity: 1, price: 299 },
        { product: product:flux_cupcake, quantity: 1, price: 120 }
    ];

CREATE order:order5 SET client = client:george, bakery = bakery:hill_valley,
    status = 'completed', created_at = d'2025-03-01T16:00:00Z', total = 255.00,
    paid_at = d'2025-03-01T16:05:00Z',
    lines = [
        { product: product:delorean_donut, quantity: 1, price: 135 },
        { product: product:flux_cupcake, quantity: 1, price: 120 }
    ];

-- Orders (Mr Cake)
CREATE order:order6 SET client = client:clara, bakery = bakery:mr_cake,
    status = 'completed', created_at = d'2025-01-10T08:30:00Z', total = 350.00,
    paid_at = d'2025-01-10T08:35:00Z',
    lines = [
        { product: product:mc_cake, quantity: 1, price: 350 }
    ];

CREATE order:order7 SET client = client:clara, bakery = bakery:mr_cake,
    status = 'shipped', created_at = d'2025-02-20T12:00:00Z', total = 275.00,
    lines = [
        { product: product:mc_croissant, quantity: 2, price: 95 },
        { product: product:mc_bread, quantity: 1, price: 85 }
    ];

CREATE order:order8 SET client = client:lorraine, bakery = bakery:mr_cake,
    status = 'completed', created_at = d'2025-03-05T10:00:00Z', total = 530.00,
    paid_at = d'2025-03-05T10:10:00Z',
    lines = [
        { product: product:mc_cake, quantity: 1, price: 350 },
        { product: product:mc_sandwich, quantity: 1, price: 180 }
    ];

CREATE order:order9 SET client = client:lorraine, bakery = bakery:mr_cake,
    status = 'completed', created_at = d'2025-03-15T15:00:00Z', total = 190.00,
    paid_at = d'2025-03-15T15:05:00Z',
    lines = [
        { product: product:mc_croissant, quantity: 2, price: 95 }
    ];

CREATE order:order10 SET client = client:biff, bakery = bakery:hill_valley,
    status = 'placed', created_at = d'2025-03-20T09:00:00Z', total = 299.00,
    lines = [
        { product: product:sea_pie, quantity: 1, price: 299 }
    ];

-- =====================================================
-- GRAPH RELATIONSHIPS
-- =====================================================

-- Client -> Bakery (primary bakery)
RELATE client:marty->primary_bakery->bakery:hill_valley;
RELATE client:doc->primary_bakery->bakery:hill_valley;
RELATE client:biff->primary_bakery->bakery:hill_valley;
RELATE client:clara->primary_bakery->bakery:mr_cake;
RELATE client:lorraine->primary_bakery->bakery:mr_cake;
RELATE client:george->primary_bakery->bakery:hill_valley;

-- Supplier -> Product (supply chain with cost data)
-- RELATE auto-sets `in` (supplier) and `out` (product) on edges.
-- We also set explicit `supplier` and `product` record link fields
-- so queries FROM supply can traverse both directions via record links.
RELATE supplier:acme_flour->supply->product:flux_cupcake SET supplier = supplier:acme_flour, product = product:flux_cupcake, unit_cost = 40.00, lead_time_days = 3;
RELATE supplier:acme_flour->supply->product:delorean_donut SET supplier = supplier:acme_flour, product = product:delorean_donut, unit_cost = 35.00, lead_time_days = 3;
RELATE supplier:acme_flour->supply->product:mc_bread SET supplier = supplier:acme_flour, product = product:mc_bread, unit_cost = 25.00, lead_time_days = 2;
RELATE supplier:acme_flour->supply->product:mc_croissant SET supplier = supplier:acme_flour, product = product:mc_croissant, unit_cost = 30.00, lead_time_days = 2;
RELATE supplier:valley_dairy->supply->product:time_tart SET supplier = supplier:valley_dairy, product = product:time_tart, unit_cost = 80.00, lead_time_days = 5;
RELATE supplier:valley_dairy->supply->product:mc_cake SET supplier = supplier:valley_dairy, product = product:mc_cake, unit_cost = 120.00, lead_time_days = 4;
RELATE supplier:sugar_works->supply->product:hover_cookies SET supplier = supplier:sugar_works, product = product:hover_cookies, unit_cost = 60.00, lead_time_days = 7;
RELATE supplier:sugar_works->supply->product:sea_pie SET supplier = supplier:sugar_works, product = product:sea_pie, unit_cost = 90.00, lead_time_days = 6;

-- Employee -> Department (works_in)
RELATE employee:alice->works_in->department:hv_backend;
RELATE employee:bob->works_in->department:hv_backend;
RELATE employee:carol->works_in->department:hv_frontend;
RELATE employee:dave->works_in->department:hv_sales;
RELATE employee:eve->works_in->department:hv_marketing;
RELATE employee:frank->works_in->department:hv_marketing;
RELATE employee:grace->works_in->department:mc_prod;
RELATE employee:hank->works_in->department:mc_qc;
RELATE employee:iris->works_in->department:mc_qc;

-- Department hierarchy (graph edges for traversal)
RELATE department:hv_frontend->sub_dept->department:hv_eng;
RELATE department:hv_backend->sub_dept->department:hv_eng;
RELATE department:hv_marketing->sub_dept->department:hv_sales;
RELATE department:mc_qc->sub_dept->department:mc_prod;
