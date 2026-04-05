// MySQL Type System (8.x+)
//
// MySQL uses static, rigid typing. A column's declared type strictly
// constrains what values it can hold. MySQL groups its types into five
// categories: numeric, date/time, string, spatial, and JSON.

vantage_type_system! {
    type_trait: MySqlType,
    method_name: mysql_value,
    value_type: mysql::Value,
    type_variants: [

        // =============================================================
        // Null
        // =============================================================

        // SQL NULL. Every nullable column can hold this sentinel value.
        //
        //   INSERT INTO t (col) VALUES (NULL);
        Null,

        // =============================================================
        // Integer types
        // =============================================================

        // 1-byte signed integer. Range: -128..127 (unsigned: 0..255).
        //
        //   CREATE TABLE sensor (reading TINYINT UNSIGNED);
        //   INSERT INTO sensor VALUES (200);
        TinyInt,

        // 2-byte signed integer. Range: -32768..32767 (unsigned: 0..65535).
        //
        //   CREATE TABLE inventory (qty SMALLINT UNSIGNED);
        //   INSERT INTO inventory VALUES (5000);
        SmallInt,

        // 3-byte signed integer. Range: -8388608..8388607.
        //
        //   CREATE TABLE stats (visits MEDIUMINT);
        //   INSERT INTO stats VALUES (1000000);
        MediumInt,

        // 4-byte signed integer. Range: ~-2.1B..2.1B (unsigned: 0..~4.3B).
        // The workhorse integer type for primary keys and general counters.
        //
        //   CREATE TABLE users (id INT AUTO_INCREMENT PRIMARY KEY);
        //   INSERT INTO users (id) VALUES (DEFAULT);
        Int,

        // 8-byte signed integer. Range: ~-9.2E18..9.2E18.
        // Used for very large counters, snowflake IDs, etc.
        //
        //   CREATE TABLE events (id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY);
        //   INSERT INTO events (id) VALUES (DEFAULT);
        BigInt,

        // =============================================================
        // Bit type
        // =============================================================

        // Fixed-length bit field of 1..64 bits. Stored as binary, displayed
        // as integer. Useful for flags and bitmasks.
        //
        //   CREATE TABLE perms (flags BIT(8));
        //   INSERT INTO perms VALUES (b'11110000');
        //   SELECT BIN(flags) FROM perms;  -- '11110000'
        Bit,

        // =============================================================
        // Boolean (alias)
        // =============================================================

        // Alias for TINYINT(1). Stores 0 (FALSE) or 1 (TRUE).
        //
        //   CREATE TABLE features (enabled BOOLEAN DEFAULT FALSE);
        //   INSERT INTO features VALUES (TRUE);
        //   SELECT * FROM features WHERE enabled IS TRUE;
        Boolean,

        // =============================================================
        // Floating-point types
        // =============================================================

        // 4-byte single-precision IEEE 754 float. Approximate values only.
        //
        //   CREATE TABLE readings (temperature FLOAT);
        //   INSERT INTO readings VALUES (36.6);
        Float,

        // 8-byte double-precision IEEE 754 float. Covers DOUBLE PRECISION
        // and REAL (unless REAL_AS_FLOAT mode is on).
        //
        //   CREATE TABLE measurements (value DOUBLE);
        //   INSERT INTO measurements VALUES (3.141592653589793);
        Double,

        // =============================================================
        // Fixed-point / exact numeric types
        // =============================================================

        // Exact fixed-point number with user-defined precision and scale.
        // DECIMAL(p, s): p total digits, s digits after the decimal point.
        // Essential for financial data. Alias: DEC, NUMERIC, FIXED.
        //
        //   CREATE TABLE ledger (amount DECIMAL(10, 2));
        //   INSERT INTO ledger VALUES (99999999.99);
        Decimal,

        // =============================================================
        // Date and time types
        // =============================================================

        // Calendar date in 'YYYY-MM-DD' format. Range: 1000-01-01..9999-12-31.
        //
        //   CREATE TABLE events (event_date DATE);
        //   INSERT INTO events VALUES ('2026-04-05');
        Date,

        // Time of day in 'HH:MM:SS[.fraction]' format.
        // Range: -838:59:59..838:59:59 (can represent elapsed time > 24h).
        //
        //   CREATE TABLE shifts (start_time TIME);
        //   INSERT INTO shifts VALUES ('08:30:00');
        Time,

        // Combined date and time in 'YYYY-MM-DD HH:MM:SS[.fraction]'.
        // Range: 1000-01-01 00:00:00..9999-12-31 23:59:59.
        //
        //   CREATE TABLE logs (created_at DATETIME(3));
        //   INSERT INTO logs VALUES ('2026-04-05 14:30:00.123');
        DateTime,

        // Unix-epoch-aware date+time. Stored as UTC, converted to/from
        // the session time zone on read/write. Range: 1970-01-01..2038-01-19.
        //
        //   CREATE TABLE audit (ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP);
        //   INSERT INTO audit (ts) VALUES (NOW());
        Timestamp,

        // 1-byte year value in 'YYYY' format. Range: 1901..2155 (or 0000).
        //
        //   CREATE TABLE movies (release_year YEAR);
        //   INSERT INTO movies VALUES (2026);
        Year,

        // =============================================================
        // String types (character)
        // =============================================================

        // Fixed-length string, right-padded with spaces. Max 255 characters.
        //
        //   CREATE TABLE codes (country_code CHAR(2));
        //   INSERT INTO codes VALUES ('GB');
        Char,

        // Variable-length string up to 65535 characters (row-size limited).
        // The most commonly used string type.
        //
        //   CREATE TABLE users (email VARCHAR(255) NOT NULL);
        //   INSERT INTO users VALUES ('alice@example.com');
        VarChar,

        // Tiny text blob, max 255 bytes.
        //
        //   CREATE TABLE notes (label TINYTEXT);
        //   INSERT INTO notes VALUES ('quick note');
        TinyText,

        // Variable-length text up to ~65 KB.
        //
        //   CREATE TABLE posts (body TEXT);
        //   INSERT INTO posts VALUES ('Lorem ipsum...');
        Text,

        // Variable-length text up to ~16 MB.
        //
        //   CREATE TABLE articles (content MEDIUMTEXT);
        //   INSERT INTO articles VALUES (REPEAT('a', 1000000));
        MediumText,

        // Variable-length text up to ~4 GB.
        //
        //   CREATE TABLE books (manuscript LONGTEXT);
        //   INSERT INTO books VALUES (LOAD_FILE('/path/to/book.txt'));
        LongText,

        // =============================================================
        // String types (enumerated / set)
        // =============================================================

        // Enumerated string column—stores one value from a predefined list.
        // Stored internally as a 1- or 2-byte integer index.
        //
        //   CREATE TABLE shirts (size ENUM('S','M','L','XL'));
        //   INSERT INTO shirts VALUES ('M');
        Enum,

        // A set of zero or more values chosen from a predefined list.
        // Stored as a bitmask (up to 64 members).
        //
        //   CREATE TABLE prefs (tags SET('news','sports','tech'));
        //   INSERT INTO prefs VALUES ('news,tech');
        Set,

        // =============================================================
        // Binary types
        // =============================================================

        // Fixed-length binary string, right-padded with 0x00. Max 255 bytes.
        //
        //   CREATE TABLE hashes (md5 BINARY(16));
        //   INSERT INTO hashes VALUES (UNHEX('d41d8cd98f00b204e9800998ecf8427e'));
        Binary,

        // Variable-length binary string up to 65535 bytes.
        //
        //   CREATE TABLE tokens (session_key VARBINARY(256));
        //   INSERT INTO tokens VALUES (RANDOM_BYTES(32));
        VarBinary,

        // Tiny binary object, max 255 bytes.
        //
        //   CREATE TABLE icons (favicon TINYBLOB);
        TinyBlob,

        // Binary object up to ~65 KB.
        //
        //   CREATE TABLE avatars (image BLOB);
        //   INSERT INTO avatars VALUES (LOAD_FILE('/path/to/pic.png'));
        Blob,

        // Binary object up to ~16 MB.
        //
        //   CREATE TABLE media (thumbnail MEDIUMBLOB);
        MediumBlob,

        // Binary object up to ~4 GB.
        //
        //   CREATE TABLE backups (archive LONGBLOB);
        LongBlob,

        // =============================================================
        // JSON type
        // =============================================================

        // Native JSON document type (since MySQL 5.7.8). Validates on write,
        // stores in an optimised binary format for fast key/index lookups.
        // Max size ~1 GB (same as LONGBLOB).
        //
        //   CREATE TABLE configs (data JSON);
        //   INSERT INTO configs VALUES ('{"theme": "dark", "lang": "en"}');
        //   SELECT data->>'$.theme' FROM configs;  -- 'dark'
        Json,

        // =============================================================
        // Spatial types (OpenGIS)
        // =============================================================

        // Abstract supertype for any geometry value. Can hold points, lines,
        // polygons, or collections thereof.
        //
        //   CREATE TABLE shapes (geom GEOMETRY SRID 4326);
        Geometry,

        // A single location in 2D coordinate space (X, Y).
        //
        //   CREATE TABLE shops (location POINT SRID 4326);
        //   INSERT INTO shops VALUES (ST_GeomFromText('POINT(51.95 -0.18)', 4326));
        Point,

        // An ordered sequence of two or more points forming a curve.
        //
        //   CREATE TABLE routes (path LINESTRING);
        //   INSERT INTO routes VALUES (ST_GeomFromText('LINESTRING(0 0, 1 1, 2 0)'));
        LineString,

        // A closed shape defined by one exterior ring and zero or more
        // interior rings (holes).
        //
        //   CREATE TABLE zones (boundary POLYGON SRID 4326);
        //   INSERT INTO zones VALUES (
        //       ST_GeomFromText('POLYGON((0 0, 4 0, 4 4, 0 4, 0 0))', 4326)
        //   );
        Polygon,

        // A collection of Point geometries.
        //
        //   CREATE TABLE clusters (pts MULTIPOINT);
        //   INSERT INTO clusters VALUES (
        //       ST_GeomFromText('MULTIPOINT((0 0),(1 1),(2 2))')
        //   );
        MultiPoint,

        // A collection of LineString geometries.
        //
        //   CREATE TABLE networks (lines MULTILINESTRING);
        MultiLineString,

        // A collection of Polygon geometries.
        //
        //   CREATE TABLE districts (areas MULTIPOLYGON);
        MultiPolygon,

        // A heterogeneous collection of any geometry types.
        //
        //   CREATE TABLE layers (mix GEOMETRYCOLLECTION);
        GeometryCollection
    ]
}
