-- =============================================================================
-- Type coercion test tables (PostgreSQL)
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

CREATE TABLE IF NOT EXISTS chrono_timestamp (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   TIMESTAMP NOT NULL
);

CREATE TABLE IF NOT EXISTS chrono_timestamptz (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   TIMESTAMPTZ NOT NULL
);

-- ── Decimal ──────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS decimal_varchar (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   VARCHAR(50) NOT NULL
);

CREATE TABLE IF NOT EXISTS decimal_numeric (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   NUMERIC(20,6) NOT NULL
);

CREATE TABLE IF NOT EXISTS decimal_numeric_wide (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   NUMERIC(38,15) NOT NULL
);

CREATE TABLE IF NOT EXISTS decimal_double (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   DOUBLE PRECISION NOT NULL
);

CREATE TABLE IF NOT EXISTS decimal_real (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS decimal_bigint (
    id      VARCHAR(50) PRIMARY KEY,
    name    VARCHAR(50) NOT NULL,
    value   BIGINT NOT NULL
);
