// PostgreSQL Type System (14+)
//
// PostgreSQL has the richest built-in type system of any major RDBMS.
// It uses strict static typing but also supports user-defined types,
// composite types, enums, domains, range types, and arrays of any type.

vantage_type_system! {
    type_trait: PostgresType,
    method_name: pg_value,
    value_type: postgres::Value,
    type_variants: [

        // =============================================================
        // Null
        // =============================================================

        // SQL NULL. Represents the absence of a value in any nullable column.
        //
        //   SELECT NULL IS NULL;  -- true
        Null,

        // =============================================================
        // Boolean
        // =============================================================

        // True boolean type (not an integer alias). Accepts TRUE/FALSE,
        // 't'/'f', 'yes'/'no', 'on'/'off', 1/0.
        //
        //   CREATE TABLE features (enabled BOOLEAN DEFAULT FALSE);
        //   INSERT INTO features VALUES (TRUE);
        //   SELECT * FROM features WHERE enabled;
        Bool,

        // =============================================================
        // Integer types
        // =============================================================

        // 2-byte signed integer. Range: -32768..32767.
        //
        //   CREATE TABLE ports (number SMALLINT CHECK (number > 0));
        //   INSERT INTO ports VALUES (8080);
        SmallInt,

        // 4-byte signed integer. Range: ~-2.1B..2.1B.
        // The default integer type in PostgreSQL.
        //
        //   CREATE TABLE users (id INTEGER GENERATED ALWAYS AS IDENTITY);
        //   INSERT INTO users DEFAULT VALUES;
        Int,

        // 8-byte signed integer. Range: ~-9.2E18..9.2E18.
        //
        //   CREATE TABLE analytics (event_id BIGINT);
        //   INSERT INTO analytics VALUES (9223372036854775807);
        BigInt,

        // =============================================================
        // Auto-incrementing serial types
        // =============================================================

        // 2-byte auto-incrementing integer (1..32767).
        // Shorthand for SMALLINT + sequence. Prefer GENERATED ALWAYS AS
        // IDENTITY in modern schemas.
        //
        //   CREATE TABLE lookup (id SMALLSERIAL PRIMARY KEY);
        SmallSerial,

        // 4-byte auto-incrementing integer (1..2147483647).
        //
        //   CREATE TABLE posts (id SERIAL PRIMARY KEY);
        //   INSERT INTO posts DEFAULT VALUES;
        Serial,

        // 8-byte auto-incrementing integer (1..9.2E18).
        //
        //   CREATE TABLE ledger (txn_id BIGSERIAL PRIMARY KEY);
        BigSerial,

        // =============================================================
        // Floating-point types
        // =============================================================

        // 4-byte single-precision IEEE 754 float. ~6 decimal digits.
        //
        //   CREATE TABLE sensors (reading REAL);
        //   INSERT INTO sensors VALUES (3.14);
        Real,

        // 8-byte double-precision IEEE 754 float. ~15 decimal digits.
        // Alias: FLOAT8.
        //
        //   CREATE TABLE experiments (result DOUBLE PRECISION);
        //   INSERT INTO experiments VALUES (2.718281828459045);
        DoublePrecision,

        // =============================================================
        // Exact numeric types
        // =============================================================

        // Arbitrary-precision exact number. NUMERIC(p, s): p total digits,
        // s fractional digits. Without parameters, stores any precision.
        // Alias: DECIMAL.
        //
        //   CREATE TABLE invoices (total NUMERIC(12, 2));
        //   INSERT INTO invoices VALUES (99999999.99);
        //   SELECT 0.1 + 0.2 = 0.3;  -- true (exact arithmetic)
        Numeric,

        // =============================================================
        // Monetary type
        // =============================================================

        // 8-byte currency amount with fixed fractional precision.
        // Locale-aware formatting. Prefer NUMERIC(p,s) for portability.
        //
        //   CREATE TABLE prices (cost MONEY);
        //   INSERT INTO prices VALUES ('$1,234.56');
        //   SELECT cost::NUMERIC FROM prices;  -- 1234.56
        Money,

        // =============================================================
        // Character types
        // =============================================================

        // Fixed-length, blank-padded string. Max 10485760 characters.
        // Rarely used in practice—VARCHAR or TEXT is preferred.
        //
        //   CREATE TABLE codes (iso CHAR(3));
        //   INSERT INTO codes VALUES ('GBR');
        Char,

        // Variable-length string with a maximum limit. Most common string type
        // for bounded inputs.
        //
        //   CREATE TABLE users (email VARCHAR(255) NOT NULL UNIQUE);
        //   INSERT INTO users VALUES ('alice@example.com');
        VarChar,

        // Variable-length string with no practical limit (~1 GB).
        // Internally identical performance to VARCHAR in PostgreSQL.
        //
        //   CREATE TABLE articles (body TEXT NOT NULL);
        //   INSERT INTO articles VALUES ('Lorem ipsum dolor sit amet...');
        Text,

        // =============================================================
        // Binary type
        // =============================================================

        // Variable-length binary data (byte array). Supports hex and
        // escape input formats. No size limit other than ~1 GB.
        //
        //   CREATE TABLE files (content BYTEA);
        //   INSERT INTO files VALUES (decode('DEADBEEF', 'hex'));
        //   INSERT INTO files VALUES ('\x44454144');
        Bytea,

        // =============================================================
        // Date and time types
        // =============================================================

        // Calendar date without time. Range: 4713 BC..5874897 AD.
        //
        //   CREATE TABLE events (event_date DATE DEFAULT CURRENT_DATE);
        //   INSERT INTO events VALUES ('2026-04-05');
        Date,

        // Time of day without date or time zone. Microsecond precision.
        //
        //   CREATE TABLE schedule (start_time TIME);
        //   INSERT INTO schedule VALUES ('14:30:00');
        Time,

        // Time of day with time zone. Microsecond precision.
        //
        //   CREATE TABLE global_schedule (call_time TIME WITH TIME ZONE);
        //   INSERT INTO global_schedule VALUES ('14:30:00+01:00');
        TimeTz,

        // Date and time without time zone. Microsecond precision.
        // Range: 4713 BC..294276 AD.
        //
        //   CREATE TABLE logs (created_at TIMESTAMP DEFAULT NOW());
        //   INSERT INTO logs VALUES ('2026-04-05 14:30:00.123456');
        Timestamp,

        // Date and time with time zone. The recommended type for most
        // timestamps. Stored as UTC internally, displayed in session TZ.
        //
        //   CREATE TABLE audit (ts TIMESTAMPTZ DEFAULT NOW());
        //   INSERT INTO audit VALUES ('2026-04-05 14:30:00+01:00');
        //   SET timezone = 'America/New_York';
        //   SELECT ts FROM audit;  -- '2026-04-05 09:30:00-04'
        TimestampTz,

        // Time span / duration. Stores months, days, and microseconds.
        //
        //   CREATE TABLE subscriptions (duration INTERVAL);
        //   INSERT INTO subscriptions VALUES ('1 year 3 months');
        //   SELECT NOW() + INTERVAL '30 days';
        Interval,

        // =============================================================
        // Bit-string types
        // =============================================================

        // Fixed-length bit string of exactly n bits.
        //
        //   CREATE TABLE masks (flags BIT(8));
        //   INSERT INTO masks VALUES (B'11001010');
        Bit,

        // Variable-length bit string up to n bits.
        //
        //   CREATE TABLE sequences (data BIT VARYING(64));
        //   INSERT INTO sequences VALUES (B'101');
        VarBit,

        // =============================================================
        // UUID
        // =============================================================

        // 128-bit universally unique identifier (RFC 4122).
        // Stored as 16 bytes, far more efficient than VARCHAR(36).
        //
        //   CREATE TABLE sessions (id UUID DEFAULT gen_random_uuid() PRIMARY KEY);
        //   INSERT INTO sessions VALUES ('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11');
        Uuid,

        // =============================================================
        // Network address types
        // =============================================================

        // IPv4 or IPv6 host address with optional subnet mask.
        //
        //   CREATE TABLE connections (client_ip INET);
        //   INSERT INTO connections VALUES ('192.168.1.1/24');
        //   INSERT INTO connections VALUES ('::1');
        Inet,

        // IPv4 or IPv6 network specification (always stores the network,
        // not the host). Rejects non-network addresses.
        //
        //   CREATE TABLE subnets (net CIDR);
        //   INSERT INTO subnets VALUES ('192.168.1.0/24');
        Cidr,

        // MAC address (6-byte Ethernet address).
        //
        //   CREATE TABLE devices (mac MACADDR);
        //   INSERT INTO devices VALUES ('08:00:2b:01:02:03');
        MacAddr,

        // MAC address in EUI-64 format (8 bytes).
        //
        //   CREATE TABLE iot (mac8 MACADDR8);
        //   INSERT INTO iot VALUES ('08:00:2b:01:02:03:04:05');
        MacAddr8,

        // =============================================================
        // JSON types
        // =============================================================

        // Stores valid JSON text. Re-parsed on every access.
        // Preserves whitespace, key order, and duplicate keys.
        //
        //   CREATE TABLE raw_events (payload JSON);
        //   INSERT INTO raw_events VALUES ('{"type": "click", "x": 100}');
        Json,

        // Binary JSON. Parsed on write, stored in a decomposed binary form.
        // Supports indexing (GIN), containment, and path queries. Preferred
        // over JSON for most use cases.
        //
        //   CREATE TABLE events (data JSONB NOT NULL);
        //   INSERT INTO events VALUES ('{"user": "alice", "action": "login"}');
        //   CREATE INDEX idx_events ON events USING GIN (data);
        //   SELECT data->>'user' FROM events;  -- 'alice'
        //   SELECT * FROM events WHERE data @> '{"action": "login"}';
        Jsonb,

        // =============================================================
        // XML type
        // =============================================================

        // Stores well-formed XML documents or content fragments.
        //
        //   CREATE TABLE feeds (entry XML);
        //   INSERT INTO feeds VALUES (
        //       XMLPARSE(DOCUMENT '<item><title>Hello</title></item>')
        //   );
        //   SELECT xpath('/item/title/text()', entry) FROM feeds;
        Xml,

        // =============================================================
        // Array type
        // =============================================================

        // Any data type can be used as a variable-length multidimensional
        // array. Arrays are first-class citizens in PostgreSQL.
        //
        //   CREATE TABLE tags (labels TEXT[]);
        //   INSERT INTO tags VALUES (ARRAY['rust', 'database', 'types']);
        //   INSERT INTO tags VALUES ('{"alpha","beta","gamma"}');
        //   SELECT labels[1] FROM tags;       -- 'rust'
        //   SELECT * FROM tags WHERE 'rust' = ANY(labels);
        Array,

        // =============================================================
        // Geometric types
        // =============================================================

        // A point in 2D space (x, y).
        //
        //   CREATE TABLE landmarks (location POINT);
        //   INSERT INTO landmarks VALUES (POINT(51.95, -0.18));
        Point,

        // Infinite line represented as {A, B, C} in Ax + By + C = 0.
        //
        //   CREATE TABLE guides (l LINE);
        //   INSERT INTO guides VALUES (LINE '{1, -1, 0}');
        Line,

        // Finite line segment defined by two endpoints.
        //
        //   CREATE TABLE edges (seg LSEG);
        //   INSERT INTO edges VALUES (LSEG '[(0,0),(1,1)]');
        Lseg,

        // Axis-aligned rectangle defined by two opposite corners.
        //
        //   CREATE TABLE bounds (area BOX);
        //   INSERT INTO bounds VALUES (BOX '((0,0),(4,4))');
        Box,

        // Open or closed path through a sequence of points.
        //
        //   CREATE TABLE trails (route PATH);
        //   INSERT INTO trails VALUES (PATH '[(0,0),(1,1),(2,0)]');  -- open
        //   INSERT INTO trails VALUES (PATH '((0,0),(1,1),(2,0))');  -- closed
        Path,

        // Closed polygon defined by a set of vertices.
        //
        //   CREATE TABLE regions (border POLYGON);
        //   INSERT INTO regions VALUES (POLYGON '((0,0),(4,0),(4,4),(0,4))');
        Polygon,

        // Circle defined by a centre point and radius.
        //
        //   CREATE TABLE zones (area CIRCLE);
        //   INSERT INTO zones VALUES (CIRCLE '<(0,0),5>');
        Circle,

        // =============================================================
        // Full-text search types
        // =============================================================

        // Sorted list of distinct normalised lexemes from a document,
        // with optional positional information. Used for full-text indexing.
        //
        //   SELECT to_tsvector('english', 'The quick brown fox');
        //   -- 'brown':3 'fox':4 'quick':2
        TsVector,

        // Parsed text-search query with boolean operators (& | ! <->).
        //
        //   SELECT to_tsquery('english', 'quick & fox');
        //   SELECT * FROM docs WHERE body_tsv @@ to_tsquery('quick & fox');
        TsQuery,

        // =============================================================
        // Range types
        // =============================================================

        // Continuous range of 4-byte integers.
        //
        //   CREATE TABLE pages (range INT4RANGE);
        //   INSERT INTO pages VALUES ('[1, 100)');
        //   SELECT * FROM pages WHERE range @> 42;
        Int4Range,

        // Continuous range of 8-byte integers.
        //
        //   CREATE TABLE id_ranges (range INT8RANGE);
        //   INSERT INTO id_ranges VALUES ('[1, 9223372036854775807)');
        Int8Range,

        // Continuous range of numeric/decimal values.
        //
        //   CREATE TABLE price_bands (range NUMRANGE);
        //   INSERT INTO price_bands VALUES ('[9.99, 49.99]');
        NumRange,

        // Continuous range of timestamps without time zone.
        //
        //   CREATE TABLE bookings (during TSRANGE);
        //   INSERT INTO bookings VALUES ('[2026-04-05 09:00, 2026-04-05 17:00)');
        TsRange,

        // Continuous range of timestamps with time zone.
        //
        //   CREATE TABLE shifts (period TSTZRANGE);
        //   INSERT INTO shifts VALUES (
        //       '[2026-04-05 09:00+01, 2026-04-05 17:00+01)'
        //   );
        TsTzRange,

        // Continuous range of dates.
        //
        //   CREATE TABLE rentals (stay DATERANGE);
        //   INSERT INTO rentals VALUES ('[2026-04-01, 2026-04-10)');
        //   SELECT * FROM rentals WHERE stay && '[2026-04-05, 2026-04-06)';
        DateRange,

        // =============================================================
        // Multirange types (PostgreSQL 14+)
        // =============================================================

        // A set of non-overlapping int4 ranges. Multiranges generalise
        // ranges to represent discontinuous sets of values.
        //
        //   SELECT '{[1,5), [10,20)}'::INT4MULTIRANGE;
        Int4MultiRange,

        // A set of non-overlapping int8 ranges.
        //
        //   SELECT '{[1,100), [200,300)}'::INT8MULTIRANGE;
        Int8MultiRange,

        // A set of non-overlapping numeric ranges.
        //
        //   SELECT '{[0.5,1.5), [3.0,4.0)}'::NUMMULTIRANGE;
        NumMultiRange,

        // A set of non-overlapping timestamp ranges.
        //
        //   SELECT '{[2026-01-01,2026-03-01), [2026-06-01,2026-09-01)}'::TSMULTIRANGE;
        TsMultiRange,

        // A set of non-overlapping timestamptz ranges.
        //
        //   SELECT '{[2026-01-01 00:00+00,2026-06-01 00:00+00)}'::TSTZMULTIRANGE;
        TsTzMultiRange,

        // A set of non-overlapping date ranges.
        //
        //   SELECT '{[2026-01-01,2026-01-31), [2026-03-01,2026-03-31)}'::DATEMULTIRANGE;
        DateMultiRange,

        // =============================================================
        // Enum type
        // =============================================================

        // User-defined enumerated type with a static ordered set of values.
        // Values are stored as 4-byte integers internally.
        //
        //   CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy');
        //   CREATE TABLE diary (day DATE, feeling mood);
        //   INSERT INTO diary VALUES ('2026-04-05', 'happy');
        //   SELECT * FROM diary WHERE feeling > 'sad';
        Enum,

        // =============================================================
        // Composite type
        // =============================================================

        // A row type / record type—a named tuple of typed fields.
        // Every table implicitly defines a composite type of the same name.
        //
        //   CREATE TYPE address AS (
        //       street TEXT, city TEXT, postcode VARCHAR(10)
        //   );
        //   CREATE TABLE contacts (name TEXT, home address);
        //   INSERT INTO contacts VALUES ('Alice', ROW('1 High St', 'Hitchin', 'SG5 1AB'));
        //   SELECT (home).city FROM contacts;  -- 'Hitchin'
        Composite,

        // =============================================================
        // Domain type
        // =============================================================

        // A named type alias with optional constraints built on top of
        // an existing base type. Useful for enforcing business rules.
        //
        //   CREATE DOMAIN email AS TEXT CHECK (VALUE ~ '^.+@.+\..+$');
        //   CREATE TABLE accounts (contact email NOT NULL);
        //   INSERT INTO accounts VALUES ('alice@example.com');  -- OK
        //   INSERT INTO accounts VALUES ('not-an-email');        -- ERROR
        Domain,

        // =============================================================
        // Object identifier types (OID)
        // =============================================================

        // 4-byte unsigned integer used internally as primary keys for
        // system catalogue tables. Rarely used in application schemas.
        //
        //   SELECT 'pg_class'::REGCLASS::OID;  -- e.g. 1259
        //   SELECT typname FROM pg_type WHERE oid = 23;  -- 'int4'
        Oid
    ]
}
