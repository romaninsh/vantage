// Hill Valley Bakery Database - MongoDB Version
// Translated from PostgreSQL v2.sql
//
// Usage: docker exec -i mongo-vantage mongosh vantage < scripts/db/v2.js

// Clean slate
db.bakery.drop();
db.client.drop();
db.product.drop();
db.client_order.drop();
db.order_line.drop();

// Bakery
db.bakery.insertOne({
  _id: "hill_valley",
  name: "Hill Valley Bakery",
  profit_margin: 15
});

// Clients
db.client.insertMany([
  {
    _id: "marty",
    name: "Marty McFly",
    email: "marty@gmail.com",
    contact_details: "555-1955",
    is_paying_client: true,
    balance: 150.00,
    bakery_id: "hill_valley"
  },
  {
    _id: "doc",
    name: "Doc Brown",
    email: "doc@brown.com",
    contact_details: "555-1885",
    is_paying_client: true,
    balance: 500.50,
    bakery_id: "hill_valley"
  },
  {
    _id: "biff",
    name: "Biff Tannen",
    email: "biff-3293@hotmail.com",
    contact_details: "555-1955",
    is_paying_client: false,
    balance: -50.25,
    bakery_id: "hill_valley"
  }
]);

// Products
db.product.insertMany([
  {
    _id: "flux_cupcake",
    name: "Flux Capacitor Cupcake",
    calories: 300,
    price: 120,
    bakery_id: "hill_valley",
    is_deleted: false,
    inventory_stock: 50,
    sticker: "cat"
  },
  {
    _id: "delorean_donut",
    name: "DeLorean Doughnut",
    calories: 250,
    price: 135,
    bakery_id: "hill_valley",
    is_deleted: false,
    inventory_stock: 30,
    sticker: "dog"
  },
  {
    _id: "time_tart",
    name: "Time Traveler Tart",
    calories: 200,
    price: 220,
    bakery_id: "hill_valley",
    is_deleted: false,
    inventory_stock: 20,
    sticker: null
  },
  {
    _id: "sea_pie",
    name: "Enchantment Under the Sea Pie",
    calories: 350,
    price: 299,
    bakery_id: "hill_valley",
    is_deleted: false,
    inventory_stock: 15,
    sticker: "pig"
  },
  {
    _id: "hover_cookies",
    name: "Hoverboard Cookies",
    calories: 150,
    price: 199,
    bakery_id: "hill_valley",
    is_deleted: false,
    inventory_stock: 40,
    sticker: "chicken"
  }
]);

// Orders
db.client_order.insertMany([
  {
    _id: "order1",
    bakery_id: "hill_valley",
    client_id: "marty",
    is_deleted: false,
    created_at: new Date().toISOString()
  },
  {
    _id: "order2",
    bakery_id: "hill_valley",
    client_id: "doc",
    is_deleted: false,
    created_at: new Date().toISOString()
  },
  {
    _id: "order3",
    bakery_id: "hill_valley",
    client_id: "doc",
    is_deleted: false,
    created_at: new Date().toISOString()
  }
]);

// Order lines
db.order_line.insertMany([
  { order_id: "order1", product_id: "flux_cupcake",   quantity: 3,   price: 120 },
  { order_id: "order1", product_id: "delorean_donut", quantity: 1,   price: 135 },
  { order_id: "order1", product_id: "hover_cookies",  quantity: 2,   price: 199 },
  { order_id: "order2", product_id: "time_tart",      quantity: 1,   price: 220 },
  { order_id: "order3", product_id: "hover_cookies",  quantity: 500, price: 199 }
]);

print("Seeded: " +
  db.bakery.countDocuments() + " bakeries, " +
  db.client.countDocuments() + " clients, " +
  db.product.countDocuments() + " products, " +
  db.client_order.countDocuments() + " orders, " +
  db.order_line.countDocuments() + " order lines"
);
