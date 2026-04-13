CREATE TABLE product (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    price INTEGER NOT NULL,
    is_deleted BOOLEAN NOT NULL DEFAULT 0
);

INSERT INTO product VALUES ('cupcake',  'Cupcake',           120, 0);
INSERT INTO product VALUES ('donut',    'Doughnut',          135, 0);
INSERT INTO product VALUES ('tart',     'Tart',              220, 0);
INSERT INTO product VALUES ('pie',      'Pie',               299, 0);
INSERT INTO product VALUES ('cookies',  'Cookies',           199, 0);
INSERT INTO product VALUES ('old_cake', 'Discontinued Cake',  80, 1);
