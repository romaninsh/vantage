-- Hill Valley Bakery Database - SurrealDB Version
-- This schema leverages SurrealDB's multi-model features including:
-- - Embedded documents (order lines within orders)
-- - Graph relationships (client->bakery, bakery->product, client->order)
-- - Record links with embedded data (inventory in products)
-- - Mixed schema modes for flexibility

-- =====================================================
-- SCHEMA DEFINITIONS
-- =====================================================

-- Bakery table (schema-full for core business data)
DEFINE TABLE bakery SCHEMAFULL;
DEFINE FIELD name ON bakery TYPE string;
DEFINE FIELD profit_margin ON bakery TYPE int ASSERT $value >= 0 AND $value <= 100;

-- Client table (schema-full with flexible metadata field)
DEFINE TABLE client SCHEMAFULL;
DEFINE FIELD email ON client TYPE string ASSERT string::is::email($value);
DEFINE FIELD name ON client TYPE string;
DEFINE FIELD contact_details ON client TYPE string;
DEFINE FIELD is_paying_client ON client TYPE bool DEFAULT false;
DEFINE FIELD bakery ON client TYPE record<bakery>; -- Direct link to bakery
DEFINE FIELD metadata ON client FLEXIBLE TYPE option<object>; -- For future extensibility
DEFINE INDEX unique_email ON client FIELDS email UNIQUE;

-- Product table (schema-full with embedded inventory)
DEFINE TABLE product SCHEMAFULL;
DEFINE FIELD name ON product TYPE string;
DEFINE FIELD calories ON product TYPE int ASSERT $value >= 0;
DEFINE FIELD price ON product TYPE int ASSERT $value > 0;
DEFINE FIELD bakery ON product TYPE record<bakery>; -- Direct link to bakery
DEFINE FIELD is_deleted ON product TYPE bool DEFAULT false;
-- Embedded inventory data
DEFINE FIELD inventory ON product TYPE object;
DEFINE FIELD inventory.stock ON product TYPE int DEFAULT 0;

-- Order table (schema-full with embedded line items)
DEFINE TABLE order SCHEMAFULL;
DEFINE FIELD bakery ON order TYPE record<bakery>; -- Direct link to bakery
DEFINE FIELD is_deleted ON order TYPE bool DEFAULT false;
DEFINE FIELD created_at ON order TYPE datetime DEFAULT time::now();
-- Embedded order lines instead of separate table
DEFINE FIELD lines ON order TYPE array<object> DEFAULT [];
DEFINE FIELD lines.*.product ON order TYPE record<product>;
DEFINE FIELD lines.*.quantity ON order TYPE int ASSERT $value > 0;
DEFINE FIELD lines.*.price ON order TYPE int ASSERT $value > 0;

-- Graph relationship tables will be automatically managed by RELATE statements

-- =====================================================
-- DATA INSERTION
-- =====================================================

-- Create bakery
CREATE bakery:hill_valley SET
    name = 'Hill Valley Bakery',
    profit_margin = 15;

-- Create clients
CREATE client:marty SET
    name = 'Marty McFly',
    email = 'marty@gmail.com',
    contact_details = '555-1955',
    is_paying_client = true,
    bakery = bakery:hill_valley;

CREATE client:doc SET
    name = 'Doc Brown',
    email = 'doc@brown.com',
    contact_details = '555-1885',
    is_paying_client = true,
    bakery = bakery:hill_valley;

CREATE client:biff SET
    name = 'Biff Tannen',
    email = 'biff-3293@hotmail.com',
    contact_details = '555-1955',
    is_paying_client = false,
    bakery = bakery:hill_valley,
    metadata = {
        notes: "Often late with payments",
        credit_limit: 500
    };

-- Create client->bakery relationships
RELATE client:marty->belongs_to->bakery:hill_valley SET joined_at = time::now();
RELATE client:doc->belongs_to->bakery:hill_valley SET joined_at = time::now();
RELATE client:biff->belongs_to->bakery:hill_valley SET joined_at = time::now();

-- Create products with embedded inventory
CREATE product:flux_cupcake SET
    name = 'Flux Capacitor Cupcake',
    calories = 300,
    price = 120,
    bakery = bakery:hill_valley,
    inventory = { stock: 50 };

CREATE product:delorean_donut SET
    name = 'DeLorean Doughnut',
    calories = 250,
    price = 135,
    bakery = bakery:hill_valley,
    inventory = { stock: 30 };

CREATE product:time_tart SET
    name = 'Time Traveler Tart',
    calories = 200,
    price = 220,
    bakery = bakery:hill_valley,
    inventory = { stock: 20 };

CREATE product:sea_pie SET
    name = 'Enchantment Under the Sea Pie',
    calories = 350,
    price = 299,
    bakery = bakery:hill_valley,
    inventory = { stock: 15 };

CREATE product:hover_cookies SET
    name = 'Hoverboard Cookies',
    calories = 150,
    price = 199,
    bakery = bakery:hill_valley,
    inventory = { stock: 40 };


-- Create orders with embedded line items
-- Order 1: Marty's order
CREATE order:order1 SET
    bakery = bakery:hill_valley,
    lines = [
        {
            product: product:flux_cupcake,
            quantity: 3,
            price: 120
        },
        {
            product: product:delorean_donut,
            quantity: 1,
            price: 135
        },
        {
            product: product:hover_cookies,
            quantity: 2,
            price: 199
        }
    ];

-- Order 2: Doc's order
CREATE order:order2 SET
    bakery = bakery:hill_valley,
    lines = [
        {
            product: product:time_tart,
            quantity: 1,
            price: 220
        }
    ];

-- Order 3: Doc's second order
CREATE order:order3 SET
    bakery = bakery:hill_valley,
    lines = [
        {
            product: product:hover_cookies,
            quantity: 500,
            price: 199
        }
    ];

-- Create client->order relationships
RELATE client:marty->placed->order:order1 SET placed_at = time::now();
RELATE client:doc->placed->order:order2 SET placed_at = time::now();
RELATE client:doc->placed->order:order3 SET placed_at = time::now();
