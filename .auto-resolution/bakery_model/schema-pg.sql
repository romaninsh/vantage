DROP TABLE IF EXISTS order_line;

DROP TABLE IF EXISTS ord;

DROP TABLE IF EXISTS product;

DROP TABLE IF EXISTS inventory;

DROP TABLE IF EXISTS client;

DROP TABLE IF EXISTS bakery;

-- Creating tables
CREATE TABLE bakery (
    id serial PRIMARY KEY,
    name varchar(255) NOT NULL,
    profit_margin int NOT NULL
);

CREATE TABLE client (
    id serial PRIMARY KEY,
    email varchar(255) UNIQUE NOT NULL,
    name varchar(255) NOT NULL,
    contact_details varchar(255) NOT NULL,
    is_paying_client BOOLEAN DEFAULT false NOT NULL,
    bakery_id int NOT NULL
);

CREATE TABLE inventory (
    product_id int PRIMARY KEY,
    stock int DEFAULT NULL
);

CREATE TABLE "ord" (
    id serial,
    product_id int NOT NULL,
    client_id int NOT NULL,
    is_deleted boolean DEFAULT false,
    PRIMARY KEY (id, client_id)
);

CREATE TABLE order_line (
    id int,
    order_id int DEFAULT NULL,
    product_id int NOT NULL,
    quantity int DEFAULT NULL,
    price int DEFAULT NULL,
    PRIMARY KEY (id, product_id)
);

CREATE TABLE product (
    id serial PRIMARY KEY,
    name varchar(255) NOT NULL,
    calories int NOT NULL,
    bakery_id int NOT NULL,
    price int NOT NULL,
    is_deleted boolean DEFAULT false
);

-- Insert data into tables
INSERT INTO
    bakery (name, profit_margin)
VALUES
    ('Hill Valley Bakery', 15);

INSERT INTO
    client (
        name,
        email,
        contact_details,
        is_paying_client,
        bakery_id
    )
VALUES
    (
        'Marty McFly',
        'marty@gmail.com',
        '555-1955',
        true,
        1
    ),
    ('Doc Brown', 'doc@brown.com', '555-1885', true, 1),
    (
        'Biff Tannen',
        'biff-3293@hotmail.com',
        '555-1955',
        false,
        1
    );

INSERT INTO
    product (name, calories, bakery_id, price)
VALUES
    ('Flux Capacitor Cupcake', 300, 1, 120),
    ('DeLorean Doughnut', 250, 1, 135),
    ('Time Traveler Tart', 200, 1, 220),
    ('Enchantment Under the Sea Pie', 350, 1, 299),
    ('Hoverboard Cookies', 150, 1, 199);

INSERT INTO
    inventory (product_id, stock)
VALUES
    (1, 50),
    (2, 30),
    (3, 20),
    (4, 15),
    (5, 40);

INSERT INTO
    "ord" (product_id, client_id)
VALUES
    (1, 1),
    (2, 2),
    (3, 2);

INSERT INTO
    order_line (id, order_id, product_id, quantity)
VALUES
    (1, 1, 1, 3),
    (2, 1, 2, 1),
    (3, 1, 5, 2),
    (4, 2, 3, 1),
    (5, 3, 5, 5);
