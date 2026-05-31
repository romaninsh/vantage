-- Hill Valley Bakery Database - SurrealDB Version
-- This schema leverages SurrealDB's multi-model features including:
-- - Embedded documents (order lines within orders)
-- - Graph relationships (client->bakery, bakery->product, client->order)
-- - Record links with embedded data (inventory in products)
-- - Mixed schema modes for flexibility

-- =====================================================
-- SCHEMA DEFINITIONS
-- =====================================================

-- Bakery table — schemaless so vantage-ui's inventory YAML is the
-- source of truth for columns; new fields land via patch_value
-- without needing a corresponding DEFINE FIELD.
DEFINE TABLE bakery SCHEMALESS;
DEFINE FIELD name ON bakery TYPE string;
DEFINE FIELD profit_margin ON bakery TYPE int ASSERT $value >= 0 AND $value <= 100;

-- Client table — schemaless (was SCHEMAFULL; lets inventory-driven
-- columns like `password_hash` land without DDL edits here).
DEFINE TABLE client SCHEMALESS;
DEFINE FIELD email ON client TYPE string;
DEFINE FIELD name ON client TYPE string;
DEFINE FIELD contact_details ON client TYPE string;
DEFINE FIELD is_paying_client ON client TYPE bool DEFAULT false;
DEFINE FIELD balance ON client TYPE decimal DEFAULT 0;
DEFINE FIELD bakery ON client TYPE record<bakery>; -- Direct link to bakery
DEFINE FIELD metadata ON client TYPE option<object> FLEXIBLE; -- For future extensibility
DEFINE INDEX unique_email ON client FIELDS email UNIQUE;

-- Product table (schema-full with embedded inventory)
DEFINE TABLE product SCHEMALESS;
DEFINE FIELD name ON product TYPE string;
DEFINE FIELD calories ON product TYPE int ASSERT $value >= 0;
DEFINE FIELD price ON product TYPE int ASSERT $value > 0;
DEFINE FIELD bakery ON product TYPE record<bakery>; -- Direct link to bakery
DEFINE FIELD is_deleted ON product TYPE bool DEFAULT false;
-- Embedded inventory data
DEFINE FIELD inventory ON product TYPE object;
DEFINE FIELD inventory.stock ON product TYPE int DEFAULT 0;

-- Order table (schema-full with embedded line items)
DEFINE TABLE order SCHEMALESS;
DEFINE FIELD client ON order TYPE record<client>; -- Direct link to client
DEFINE FIELD bakery ON order TYPE record<bakery>; -- Direct link to bakery
DEFINE FIELD is_deleted ON order TYPE bool DEFAULT false;
DEFINE FIELD created_at ON order TYPE datetime DEFAULT time::now();
-- Lifecycle state + cancellation metadata. Catalog YAML
-- (`examples/surreal-bakery/table/bakery-surreal/order.yaml`)
-- carries the human-readable choices. `option<string>` so old rows
-- without a cancellation reason don't trip the type.
DEFINE FIELD status ON order TYPE string DEFAULT "placed"
    ASSERT $value IN ["placed", "confirmed", "in_production", "ready",
                      "delivered", "picked_up", "paid", "cancelled"];
DEFINE FIELD cancellation_reason ON order TYPE option<string>;
DEFINE FIELD cancellation_note ON order TYPE option<string>;
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
    balance = 150.00,
    bakery = bakery:hill_valley;

CREATE client:doc SET
    name = 'Doc Brown',
    email = 'doc@brown.com',
    contact_details = '555-1885',
    is_paying_client = true,
    balance = 500.50,
    bakery = bakery:hill_valley;

CREATE client:biff SET
    name = 'Biff Tannen',
    email = 'biff-3293@hotmail.com',
    contact_details = '555-1955',
    is_paying_client = false,
    balance = -50.25,
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


-- Create orders with embedded line items. Status values seed the
-- lifecycle at different points so the right-click menu demos every
-- transition gate (placed → confirmed → in_production → ready →
-- delivered/picked_up → paid).
-- Order 1: Marty's order — fresh placement
CREATE order:order1 SET
    client = client:marty,
    bakery = bakery:hill_valley,
    status = "placed",
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

-- Order 2: Doc's order — mid-flight (ready for delivery / pickup)
CREATE order:order2 SET
    client = client:doc,
    bakery = bakery:hill_valley,
    status = "ready",
    lines = [
        {
            product: product:time_tart,
            quantity: 1,
            price: 220
        }
    ];

-- Order 3: Doc's second order — delivered, awaiting payment
CREATE order:order3 SET
    client = client:doc,
    bakery = bakery:hill_valley,
    status = "delivered",
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
