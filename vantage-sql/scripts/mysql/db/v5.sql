-- =============================================================================
-- Type coercion test tables
-- Same shape (id, name, value) with different column types for `value`
-- =============================================================================

-- ── Chrono ───────────────────────────────────────────────────────────────

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

-- ── Chrono with fractional seconds ──────────────────────────────────────

CREATE TABLE IF NOT EXISTS chrono_time6 (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   TIME(6) NOT NULL
);

CREATE TABLE IF NOT EXISTS chrono_datetime6 (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   DATETIME(6) NOT NULL
);

CREATE TABLE IF NOT EXISTS chrono_timestamp6 (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   TIMESTAMP(6) NOT NULL
);

-- ── Decimal ──────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS decimal_varchar (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   VARCHAR(50) NOT NULL
);

CREATE TABLE IF NOT EXISTS decimal_decimal (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   DECIMAL(20,6) NOT NULL
);

CREATE TABLE IF NOT EXISTS decimal_decimal_wide (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   DECIMAL(38,15) NOT NULL
);

CREATE TABLE IF NOT EXISTS decimal_double (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   DOUBLE NOT NULL
);

CREATE TABLE IF NOT EXISTS decimal_float (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   FLOAT NOT NULL
);

CREATE TABLE IF NOT EXISTS decimal_bigint (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   BIGINT NOT NULL
);
