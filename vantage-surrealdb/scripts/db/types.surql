-- SurrealDB Types Testing Database
-- Minimal working schema for testing basic types

-- Define simple tables for testing

-- Strings table
DEFINE TABLE strings;

-- Numbers table
DEFINE TABLE numbers;

-- Booleans table
DEFINE TABLE booleans;

-- Mixed table
DEFINE TABLE mixed;

-- Insert test data

-- String data
INSERT INTO strings { id: "str1", value: "hello world" };
INSERT INTO strings { id: "str2", value: "test string" };

-- Number data
INSERT INTO numbers { id: "num1", int_val: 42, float_val: 3.14 };
INSERT INTO numbers { id: "num2", int_val: -123, float_val: 2.718 };

-- Boolean data
INSERT INTO booleans { id: "bool1", value: true };
INSERT INTO booleans { id: "bool2", value: false };

-- Mixed data
INSERT INTO mixed {
    id: "mix1",
    name: "Test Record",
    age: 25,
    active: true,
    score: 95.5
};
