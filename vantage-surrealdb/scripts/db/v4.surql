-- Hill Valley Bakery — comprehensive seed for COMPLEX query exercises.
--
-- Where v1/v2 cover the basics, v4 is deliberately rich so that the
-- companion tests/v4.surql can lean on SurrealDB's advanced surface:
--   - deep graph traversal + recursion (org chart, referral chains)
--   - edge properties (review ratings live on the `reviewed` edge)
--   - embedded line items with closures / array::fold / array::map
--   - GROUP BY with the math:: stats family
--   - time::group bucketing over a year of orders
--   - computed fields, object:: functions, string similarity, SPLIT
--
-- Everything hangs off a single bakery so the dataset stays self-consistent
-- and every query returns rows.

-- =====================================================
-- SCHEMA DEFINITIONS
-- =====================================================

DEFINE TABLE bakery SCHEMALESS;
DEFINE FIELD name ON bakery TYPE string;
DEFINE FIELD profit_margin ON bakery TYPE int ASSERT $value >= 0 AND $value <= 100;

-- Departments form a tree via the `part_of` graph edge (pastry -> production
-- -> exec), which the recursion test walks downward.
DEFINE TABLE department SCHEMALESS;
DEFINE FIELD name ON department TYPE string;
DEFINE FIELD cost_center ON department TYPE string;

-- Employees carry both a direct `department` link (easy GROUP BY) and a
-- `reports_to` graph edge (recursive org chart). Salaries are spread out so
-- the stats functions produce interesting spreads.
DEFINE TABLE employee SCHEMALESS;
DEFINE FIELD name ON employee TYPE string;
DEFINE FIELD title ON employee TYPE string;
DEFINE FIELD salary ON employee TYPE int ASSERT $value > 0;
DEFINE FIELD hired ON employee TYPE datetime;
DEFINE FIELD department ON employee TYPE record<department>;

-- Clients gain a loyalty tier and signup date for banding / cohort queries.
DEFINE TABLE client SCHEMALESS;
DEFINE FIELD name ON client TYPE string;
DEFINE FIELD email ON client TYPE string;
DEFINE FIELD contact_details ON client TYPE string;
DEFINE FIELD is_paying_client ON client TYPE bool DEFAULT false;
DEFINE FIELD balance ON client TYPE decimal DEFAULT 0;
DEFINE FIELD tier ON client TYPE string DEFAULT "bronze"
    ASSERT $value IN ["bronze", "silver", "gold"];
DEFINE FIELD signup ON client TYPE datetime;
DEFINE FIELD bakery ON client TYPE record<bakery>;
DEFINE FIELD metadata ON client TYPE option<object>;
DEFINE INDEX unique_email ON client FIELDS email UNIQUE;

-- Products get a category, a tags array (for SPLIT) and a nutrition object
-- (for object:: functions).
DEFINE TABLE product SCHEMALESS;
DEFINE FIELD name ON product TYPE string;
DEFINE FIELD category ON product TYPE string;
DEFINE FIELD calories ON product TYPE int ASSERT $value >= 0;
DEFINE FIELD price ON product TYPE int ASSERT $value > 0;
DEFINE FIELD tags ON product TYPE array<string> DEFAULT [];
DEFINE FIELD nutrition ON product TYPE object;
DEFINE FIELD bakery ON product TYPE record<bakery>;
DEFINE FIELD is_deleted ON product TYPE bool DEFAULT false;
DEFINE FIELD inventory ON product TYPE object;
DEFINE FIELD inventory.stock ON product TYPE int DEFAULT 0;

-- Orders embed their line items. `total` is a COMPUTED field — never stored,
-- recalculated from the lines on every read — so revenue queries don't have
-- to re-derive it.
DEFINE TABLE order SCHEMALESS;
DEFINE FIELD client ON order TYPE record<client>;
DEFINE FIELD bakery ON order TYPE record<bakery>;
DEFINE FIELD status ON order TYPE string DEFAULT "placed"
    ASSERT $value IN ["placed", "confirmed", "in_production", "ready",
                      "delivered", "picked_up", "paid", "cancelled"];
DEFINE FIELD created_at ON order TYPE datetime DEFAULT time::now();
DEFINE FIELD is_deleted ON order TYPE bool DEFAULT false;
DEFINE FIELD lines ON order TYPE array<object> DEFAULT [];
DEFINE FIELD lines.*.product ON order TYPE record<product>;
DEFINE FIELD lines.*.quantity ON order TYPE int ASSERT $value > 0;
DEFINE FIELD lines.*.price ON order TYPE int ASSERT $value > 0;
DEFINE FIELD total ON order TYPE int
    COMPUTED math::sum(lines.map(|$l| $l.quantity * $l.price));

-- =====================================================
-- DATA: bakery
-- =====================================================

CREATE bakery:hill_valley SET
    name = 'Hill Valley Bakery',
    profit_margin = 15;

-- =====================================================
-- DATA: departments (tree)
-- =====================================================

CREATE department:exec       SET name = 'Executive',       cost_center = 'CC-100';
CREATE department:production SET name = 'Production',       cost_center = 'CC-200';
CREATE department:front      SET name = 'Front of House',   cost_center = 'CC-300';
CREATE department:pastry     SET name = 'Pastry',           cost_center = 'CC-210';
CREATE department:bread      SET name = 'Bread',            cost_center = 'CC-220';
CREATE department:sales      SET name = 'Sales',            cost_center = 'CC-310';
CREATE department:delivery   SET name = 'Delivery',         cost_center = 'CC-320';

-- department tree: leaf ->part_of-> parent
RELATE department:production->part_of->department:exec;
RELATE department:front->part_of->department:exec;
RELATE department:pastry->part_of->department:production;
RELATE department:bread->part_of->department:production;
RELATE department:sales->part_of->department:front;
RELATE department:delivery->part_of->department:front;

-- =====================================================
-- DATA: employees + org chart
-- =====================================================

CREATE employee:george    SET name = 'George McFly',      title = 'Owner',           salary = 95000, hired = d'2020-01-15T00:00:00Z', department = department:exec;
CREATE employee:lorraine  SET name = 'Lorraine Baines',   title = 'Head of Production', salary = 72000, hired = d'2020-03-01T00:00:00Z', department = department:production;
CREATE employee:goldie    SET name = 'Goldie Wilson',     title = 'Head of Front',   salary = 68000, hired = d'2020-02-10T00:00:00Z', department = department:front;
CREATE employee:dave      SET name = 'Dave McFly',        title = 'Pastry Chef',     salary = 51000, hired = d'2021-06-01T00:00:00Z', department = department:pastry;
CREATE employee:linda     SET name = 'Linda McFly',       title = 'Baker',           salary = 49000, hired = d'2021-07-15T00:00:00Z', department = department:bread;
CREATE employee:strickland SET name = 'Gerald Strickland', title = 'Sales Lead',     salary = 55000, hired = d'2021-09-01T00:00:00Z', department = department:sales;
CREATE employee:jennifer  SET name = 'Jennifer Parker',   title = 'Sales Associate', salary = 42000, hired = d'2022-01-10T00:00:00Z', department = department:sales;
CREATE employee:needles   SET name = 'Douglas Needles',   title = 'Driver',          salary = 38000, hired = d'2022-03-20T00:00:00Z', department = department:delivery;
CREATE employee:copernicus SET name = 'Copernicus Brown', title = 'Apprentice',      salary = 36000, hired = d'2023-02-01T00:00:00Z', department = department:pastry;

-- reports_to chain (subordinate ->reports_to-> manager)
RELATE employee:lorraine->reports_to->employee:george;
RELATE employee:goldie->reports_to->employee:george;
RELATE employee:dave->reports_to->employee:lorraine;
RELATE employee:linda->reports_to->employee:lorraine;
RELATE employee:strickland->reports_to->employee:goldie;
RELATE employee:jennifer->reports_to->employee:goldie;
RELATE employee:needles->reports_to->employee:goldie;
RELATE employee:copernicus->reports_to->employee:dave;

-- =====================================================
-- DATA: products
-- =====================================================

CREATE product:flux_cupcake SET
    name = 'Flux Capacitor Cupcake', category = 'pastry', calories = 300, price = 120,
    tags = ['signature', 'chocolate'], nutrition = { protein: 4, sugar: 22, fat: 12 },
    bakery = bakery:hill_valley, inventory = { stock: 50 };
CREATE product:delorean_donut SET
    name = 'DeLorean Doughnut', category = 'pastry', calories = 250, price = 135,
    tags = ['signature', 'glazed'], nutrition = { protein: 5, sugar: 18, fat: 14 },
    bakery = bakery:hill_valley, inventory = { stock: 30 };
CREATE product:time_tart SET
    name = 'Time Traveler Tart', category = 'tart', calories = 200, price = 220,
    tags = ['fruit', 'seasonal'], nutrition = { protein: 3, sugar: 24, fat: 8 },
    bakery = bakery:hill_valley, inventory = { stock: 20 };
CREATE product:sea_pie SET
    name = 'Enchantment Under the Sea Pie', category = 'pie', calories = 350, price = 299,
    tags = ['savory', 'seasonal'], nutrition = { protein: 9, sugar: 6, fat: 20 },
    bakery = bakery:hill_valley, inventory = { stock: 15 };
CREATE product:hover_cookies SET
    name = 'Hoverboard Cookies', category = 'cookie', calories = 150, price = 199,
    tags = ['chocolate', 'kids'], nutrition = { protein: 2, sugar: 16, fat: 9 },
    bakery = bakery:hill_valley, inventory = { stock: 40 };
CREATE product:plutonium_pretzel SET
    name = 'Plutonium Pretzel', category = 'bread', calories = 180, price = 95,
    tags = ['savory'], nutrition = { protein: 6, sugar: 2, fat: 4 },
    bakery = bakery:hill_valley, inventory = { stock: 60 };
CREATE product:clocktower_cake SET
    name = 'Clock Tower Cake', category = 'cake', calories = 800, price = 450,
    tags = ['signature', 'celebration'], nutrition = { protein: 8, sugar: 60, fat: 32 },
    bakery = bakery:hill_valley, inventory = { stock: 8 };
CREATE product:einstein_eclair SET
    name = 'Einstein Eclair', category = 'pastry', calories = 290, price = 145,
    tags = ['chocolate', 'classic'], nutrition = { protein: 5, sugar: 20, fat: 15 },
    bakery = bakery:hill_valley, inventory = { stock: 25 };
CREATE product:mr_fusion_muffin SET
    name = 'Mr Fusion Muffin', category = 'pastry', calories = 240, price = 110,
    tags = ['fruit', 'classic'], nutrition = { protein: 4, sugar: 14, fat: 10 },
    bakery = bakery:hill_valley, inventory = { stock: 35 };

RELATE bakery:hill_valley->owns->product:flux_cupcake;
RELATE bakery:hill_valley->owns->product:delorean_donut;
RELATE bakery:hill_valley->owns->product:time_tart;
RELATE bakery:hill_valley->owns->product:sea_pie;
RELATE bakery:hill_valley->owns->product:hover_cookies;
RELATE bakery:hill_valley->owns->product:plutonium_pretzel;
RELATE bakery:hill_valley->owns->product:clocktower_cake;
RELATE bakery:hill_valley->owns->product:einstein_eclair;
RELATE bakery:hill_valley->owns->product:mr_fusion_muffin;

-- =====================================================
-- DATA: clients
-- =====================================================

CREATE client:marty SET
    name = 'Marty McFly', email = 'marty@gmail.com', contact_details = '555-1955',
    is_paying_client = true, balance = 150.00, tier = 'gold',
    signup = d'2021-05-01T00:00:00Z', bakery = bakery:hill_valley;
CREATE client:doc SET
    name = 'Emmett Brown', email = 'doc@brown.com', contact_details = '555-1885',
    is_paying_client = true, balance = 500.50, tier = 'gold',
    signup = d'2020-11-15T00:00:00Z', bakery = bakery:hill_valley;
CREATE client:biff SET
    name = 'Biff Tannen', email = 'biff-3293@hotmail.com', contact_details = '555-1955',
    is_paying_client = false, balance = -50.25, tier = 'bronze',
    signup = d'2022-08-20T00:00:00Z', bakery = bakery:hill_valley,
    metadata = { notes: "Often late with payments", credit_limit: 500 };
CREATE client:clara SET
    name = 'Clara Clayton', email = 'clara@hilldale.com', contact_details = '555-1885',
    is_paying_client = true, balance = 75.00, tier = 'silver',
    signup = d'2023-01-10T00:00:00Z', bakery = bakery:hill_valley;
CREATE client:seamus SET
    name = 'Seamus McFly', email = 'seamus@hilldale.com', contact_details = '555-1885',
    is_paying_client = true, balance = 30.00, tier = 'silver',
    signup = d'2023-03-05T00:00:00Z', bakery = bakery:hill_valley;
CREATE client:einstein SET
    name = 'Einstein', email = 'einstein@brown.com', contact_details = '555-1885',
    is_paying_client = false, balance = 0.00, tier = 'bronze',
    signup = d'2023-06-01T00:00:00Z', bakery = bakery:hill_valley;

RELATE client:marty->belongs_to->bakery:hill_valley SET joined_at = time::now();
RELATE client:doc->belongs_to->bakery:hill_valley SET joined_at = time::now();
RELATE client:biff->belongs_to->bakery:hill_valley SET joined_at = time::now();
RELATE client:clara->belongs_to->bakery:hill_valley SET joined_at = time::now();
RELATE client:seamus->belongs_to->bakery:hill_valley SET joined_at = time::now();
RELATE client:einstein->belongs_to->bakery:hill_valley SET joined_at = time::now();

-- referral graph: referrer ->referred-> referee
RELATE client:marty->referred->client:biff SET on = d'2022-08-20T00:00:00Z';
RELATE client:doc->referred->client:clara SET on = d'2023-01-10T00:00:00Z';
RELATE client:clara->referred->client:seamus SET on = d'2023-03-05T00:00:00Z';
RELATE client:doc->referred->client:einstein SET on = d'2023-06-01T00:00:00Z';

-- =====================================================
-- DATA: orders (a year of activity, embedded line items)
-- =====================================================

CREATE order:order1 SET client = client:marty, bakery = bakery:hill_valley, status = 'paid',
    created_at = d'2024-01-15T10:30:00Z', lines = [
        { product: product:flux_cupcake,   quantity: 3, price: 120 },
        { product: product:delorean_donut, quantity: 1, price: 135 },
        { product: product:hover_cookies,  quantity: 2, price: 199 }
    ];
CREATE order:order2 SET client = client:doc, bakery = bakery:hill_valley, status = 'ready',
    created_at = d'2024-01-22T09:15:00Z', lines = [
        { product: product:time_tart,      quantity: 1, price: 220 },
        { product: product:einstein_eclair, quantity: 2, price: 145 }
    ];
CREATE order:order3 SET client = client:doc, bakery = bakery:hill_valley, status = 'delivered',
    created_at = d'2024-02-10T14:00:00Z', lines = [
        { product: product:hover_cookies,  quantity: 5, price: 199 }
    ];
CREATE order:order4 SET client = client:biff, bakery = bakery:hill_valley, status = 'paid',
    created_at = d'2024-02-18T16:45:00Z', lines = [
        { product: product:sea_pie,           quantity: 1, price: 299 },
        { product: product:plutonium_pretzel, quantity: 4, price: 95 }
    ];
CREATE order:order5 SET client = client:clara, bakery = bakery:hill_valley, status = 'paid',
    created_at = d'2024-03-05T11:20:00Z', lines = [
        { product: product:clocktower_cake, quantity: 1, price: 450 }
    ];
CREATE order:order6 SET client = client:marty, bakery = bakery:hill_valley, status = 'delivered',
    created_at = d'2024-03-20T13:10:00Z', lines = [
        { product: product:flux_cupcake,    quantity: 2, price: 120 },
        { product: product:mr_fusion_muffin, quantity: 3, price: 110 }
    ];
CREATE order:order7 SET client = client:seamus, bakery = bakery:hill_valley, status = 'placed',
    created_at = d'2024-04-02T08:50:00Z', lines = [
        { product: product:plutonium_pretzel, quantity: 6, price: 95 },
        { product: product:hover_cookies,     quantity: 2, price: 199 }
    ];
CREATE order:order8 SET client = client:doc, bakery = bakery:hill_valley, status = 'confirmed',
    created_at = d'2024-04-15T15:30:00Z', lines = [
        { product: product:time_tart, quantity: 2, price: 220 },
        { product: product:sea_pie,   quantity: 1, price: 299 }
    ];
CREATE order:order9 SET client = client:clara, bakery = bakery:hill_valley, status = 'paid',
    created_at = d'2024-05-01T10:05:00Z', lines = [
        { product: product:einstein_eclair, quantity: 4, price: 145 }
    ];
CREATE order:order10 SET client = client:marty, bakery = bakery:hill_valley, status = 'paid',
    created_at = d'2024-05-12T12:40:00Z', lines = [
        { product: product:clocktower_cake, quantity: 1, price: 450 },
        { product: product:flux_cupcake,    quantity: 4, price: 120 }
    ];

RELATE client:marty->placed->order:order1 SET placed_at = d'2024-01-15T10:30:00Z';
RELATE client:doc->placed->order:order2 SET placed_at = d'2024-01-22T09:15:00Z';
RELATE client:doc->placed->order:order3 SET placed_at = d'2024-02-10T14:00:00Z';
RELATE client:biff->placed->order:order4 SET placed_at = d'2024-02-18T16:45:00Z';
RELATE client:clara->placed->order:order5 SET placed_at = d'2024-03-05T11:20:00Z';
RELATE client:marty->placed->order:order6 SET placed_at = d'2024-03-20T13:10:00Z';
RELATE client:seamus->placed->order:order7 SET placed_at = d'2024-04-02T08:50:00Z';
RELATE client:doc->placed->order:order8 SET placed_at = d'2024-04-15T15:30:00Z';
RELATE client:clara->placed->order:order9 SET placed_at = d'2024-05-01T10:05:00Z';
RELATE client:marty->placed->order:order10 SET placed_at = d'2024-05-12T12:40:00Z';

-- =====================================================
-- DATA: reviews (ratings live on the `reviewed` edge)
-- =====================================================

RELATE client:marty->reviewed->product:flux_cupcake SET rating = 5, comment = 'Heavy! Like 1.21 gigawatts of flavour', reviewed_at = d'2024-01-20T00:00:00Z';
RELATE client:doc->reviewed->product:flux_cupcake SET rating = 4, comment = 'Great Scott, delicious', reviewed_at = d'2024-01-25T00:00:00Z';
RELATE client:biff->reviewed->product:flux_cupcake SET rating = 2, comment = 'Make like a tree', reviewed_at = d'2024-02-01T00:00:00Z';
RELATE client:clara->reviewed->product:clocktower_cake SET rating = 5, comment = 'Saved the day', reviewed_at = d'2024-03-10T00:00:00Z';
RELATE client:marty->reviewed->product:clocktower_cake SET rating = 4, comment = 'A bit much', reviewed_at = d'2024-05-15T00:00:00Z';
RELATE client:doc->reviewed->product:time_tart SET rating = 3, comment = 'Temporal aftertaste', reviewed_at = d'2024-01-30T00:00:00Z';
RELATE client:seamus->reviewed->product:hover_cookies SET rating = 5, comment = 'The kids love them', reviewed_at = d'2024-04-10T00:00:00Z';
RELATE client:clara->reviewed->product:einstein_eclair SET rating = 4, comment = 'Relatively good', reviewed_at = d'2024-05-05T00:00:00Z';
RELATE client:biff->reviewed->product:sea_pie SET rating = 1, comment = 'Butthead', reviewed_at = d'2024-02-25T00:00:00Z';
RELATE client:doc->reviewed->product:einstein_eclair SET rating = 5, comment = 'My favourite', reviewed_at = d'2024-05-06T00:00:00Z';
