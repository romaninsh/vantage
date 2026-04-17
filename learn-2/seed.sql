CREATE TABLE product (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    price INTEGER NOT NULL,
    category_id INTEGER,
    is_deleted BOOLEAN NOT NULL DEFAULT 0
);

INSERT INTO product VALUES (1, 'Cupcake',           120, 1, 0);
INSERT INTO product VALUES (2, 'Doughnut',          135, 1, 0);
INSERT INTO product VALUES (3, 'Tart',              220, 2, 0);
INSERT INTO product VALUES (4, 'Pie',               299, 2, 0);
INSERT INTO product VALUES (5, 'Cookies',           199, 1, 0);
INSERT INTO product VALUES (6, 'A Stale Cake',  80, 1, 1);
INSERT INTO product VALUES (7, 'Sourdough Loaf',    350, 3, 0);

CREATE TABLE category (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

INSERT INTO category VALUES (1, 'Sweet Treats');
INSERT INTO category VALUES (2, 'Pastries');
INSERT INTO category VALUES (3, 'Breads');
