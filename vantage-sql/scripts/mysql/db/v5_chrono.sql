-- =============================================================================
-- Chrono type coercion test tables
-- 5 tables, same shape (id, name, value), different column type for `value`
-- =============================================================================

CREATE TABLE IF NOT EXISTS chrono_varchar (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   VARCHAR(50) NOT NULL
);

CREATE TABLE IF NOT EXISTS chrono_date (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   DATE NOT NULL
);

CREATE TABLE IF NOT EXISTS chrono_time (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   TIME NOT NULL
);

CREATE TABLE IF NOT EXISTS chrono_datetime (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   DATETIME NOT NULL
);

CREATE TABLE IF NOT EXISTS chrono_timestamp (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   TIMESTAMP NOT NULL
);
