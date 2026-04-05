// SQLite Type System
//
// SQLite uses a dynamic type system with 5 storage classes.
// Unlike most SQL databases, the type is associated with the *value*
// itself, not the column. Any column can store any type (unless STRICT).
// Type "affinity" is merely a preference hint, not an enforcement.

vantage_type_system! {
    type_trait: SqliteType,
    method_name: sqlite_value,
    value_type: sqlite::Value,
    type_variants: [

        // =====================================================================
        // Null
        // =====================================================================

        // The NULL storage class. Represents the absence of any value.
        // Unlike most databases, any column in SQLite can hold NULL by default—
        // there is no separate nullable wrapper.
        //
        //   INSERT INTO t VALUES (NULL);
        //   SELECT typeof(NULL);  -- 'null'
        Null,

        // =====================================================================
        // Integer types
        // =====================================================================

        // Signed integer stored in 1, 2, 3, 4, 6, or 8 bytes depending on
        // the magnitude of the value. Covers TINYINT, SMALLINT, MEDIUMINT,
        // INT, INTEGER, and BIGINT—SQLite treats them all identically.
        //
        //   CREATE TABLE t (id INTEGER PRIMARY KEY, count INT);
        //   INSERT INTO t VALUES (1, 42);
        //   SELECT typeof(42);  -- 'integer'
        Integer,

        // =====================================================================
        // Floating-point types
        // =====================================================================

        // 8-byte IEEE 754 floating point. Covers REAL, DOUBLE, DOUBLE PRECISION,
        // and FLOAT. SQLite stores all floating-point values as 8-byte reals—
        // there is no single-precision float.
        //
        //   CREATE TABLE measurements (temp REAL);
        //   INSERT INTO measurements VALUES (36.6);
        //   SELECT typeof(36.6);  -- 'real'
        Real,

        // =====================================================================
        // Text types
        // =====================================================================

        // Variable-length UTF-8, UTF-16BE, or UTF-16LE text string.
        // Covers TEXT, VARCHAR, CHAR, CLOB, NCHAR, NVARCHAR, and
        // VARYING CHARACTER. Length constraints (e.g. VARCHAR(255)) are
        // accepted syntactically but *not enforced* by SQLite.
        //
        //   CREATE TABLE users (name TEXT, bio VARCHAR(500));
        //   INSERT INTO users VALUES ('Alice', 'Software engineer');
        //   SELECT typeof('hello');  -- 'text'
        Text,

        // =====================================================================
        // Binary types
        // =====================================================================

        // Raw binary data stored exactly as input, with no encoding conversion.
        // Used for images, serialised objects, or any opaque byte sequence.
        // No size limit other than SQLITE_MAX_LENGTH (default ~1 GB).
        //
        //   CREATE TABLE files (data BLOB);
        //   INSERT INTO files VALUES (x'DEADBEEF');
        //   SELECT typeof(x'0010');  -- 'blob'
        Blob,

        // =====================================================================
        // Numeric affinity (virtual / composite)
        // =====================================================================

        // The NUMERIC type affinity. Not a distinct storage class—values with
        // NUMERIC affinity are stored as INTEGER or REAL if they look like
        // valid numbers, otherwise stored as TEXT. Covers NUMERIC, DECIMAL,
        // BOOLEAN, DATE, and DATETIME declared types.
        //
        // Boolean values are stored as INTEGER 0 (false) or 1 (true):
        //   INSERT INTO flags VALUES (TRUE);   -- stored as integer 1
        //   INSERT INTO flags VALUES (FALSE);  -- stored as integer 0
        //
        // Date/time values have no dedicated type; they are stored as TEXT
        // (ISO-8601), REAL (Julian day), or INTEGER (Unix epoch) depending
        // on the function used:
        //   SELECT date('now');                -- '2026-04-05' (TEXT)
        //   SELECT julianday('now');           -- 2461402.5    (REAL)
        //   SELECT unixepoch('now');           -- 1775520000   (INTEGER)
        Numeric
    ]
}
