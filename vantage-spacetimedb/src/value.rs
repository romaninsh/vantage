//! Decoding rows, and the SATS ↔ CBOR value boundary.
//!
//! `POST /v1/database/:db/sql` answers with one envelope per statement:
//!
//! ```json
//! [{ "schema": { "elements": [ … ] }, "rows": [ [1, "playing", 75, [0, 1]] ] }]
//! ```
//!
//! Rows are **positional arrays** matching `schema.elements`, and sums encode as
//! `[variant_index, value]` — so `Some(1)` is `[0, 1]` and `None` is `[1, []]`.
//! Nothing about that is guessable from a row alone: the same `[0, 1]` could be
//! a two-element array. Decoding therefore has to be *seeded* with the schema,
//! which is why this goes through SATS's own seeded deserializer rather than
//! reading `serde_json::Value` and hoping.
//!
//! Vista speaks `ciborium::Value`, so the second half of this module maps
//! `AlgebraicValue` onto CBOR. The mapping is deliberately lossy in one
//! direction — see [`algebraic_to_cbor`].

use ciborium::Value as CborValue;
use serde::Deserialize;
use spacetimedb_lib::de::serde::SeedWrapper;
use spacetimedb_sats::{AlgebraicValue, ProductType, ProductValue, Typespace};
use vantage_core::{Result, error};
use vantage_types::Record;

/// One statement's worth of results, as the host returns it.
#[derive(Deserialize)]
struct SqlResult<'a> {
    #[serde(borrow)]
    schema: &'a serde_json::value::RawValue,
    #[serde(borrow, default)]
    rows: Vec<&'a serde_json::value::RawValue>,
}

/// Decoded rows plus the column names they line up with.
#[derive(Debug)]
pub struct DecodedRows {
    pub columns: Vec<String>,
    pub rows: Vec<ProductValue>,
}

/// Decode the response to a single-statement query.
///
/// Multi-statement responses return the **last** result, matching what a caller
/// issuing `stmt1; stmt2` expects to get back.
pub fn decode_sql_response(body: &str) -> Result<DecodedRows> {
    let results: Vec<SqlResult> = serde_json::from_str(body).map_err(|e| {
        error!(
            "could not parse the SpacetimeDB SQL response envelope",
            error = e.to_string(),
            body = truncate(body)
        )
    })?;

    let Some(result) = results.last() else {
        // A statement that returns nothing (a DML write) yields no envelope.
        return Ok(DecodedRows {
            columns: Vec::new(),
            rows: Vec::new(),
        });
    };

    let schema: ProductType = {
        let wrapper: spacetimedb_sats::serde::SerdeWrapper<ProductType> =
            serde_json::from_str(schema_str(result)).map_err(|e| {
                error!(
                    "could not parse the row schema in a SpacetimeDB SQL response",
                    error = e.to_string()
                )
            })?;
        wrapper.0
    };

    let columns: Vec<String> = schema
        .elements
        .iter()
        .enumerate()
        .map(|(i, e)| {
            e.name()
                .map(|n| n.to_string())
                // A projection can produce unnamed columns (`SELECT COUNT(*)`
                // without an alias). Positional names keep those addressable
                // instead of collapsing them all to one key.
                .unwrap_or_else(|| format!("column_{i}"))
        })
        .collect();

    // Seeded decode: the schema tells the deserializer what each position means.
    let ty = Typespace::EMPTY.with_type(&schema);
    let mut rows = Vec::with_capacity(result.rows.len());
    for (i, raw) in result.rows.iter().enumerate() {
        let mut de = serde_json::Deserializer::from_str(raw.get());
        let value: ProductValue = serde::de::DeserializeSeed::deserialize(SeedWrapper(ty), &mut de)
            .map_err(|e| {
                error!(
                    "could not decode a row against its schema",
                    row = i as i64,
                    error = e.to_string()
                )
            })?;
        rows.push(value);
    }

    Ok(DecodedRows { columns, rows })
}

fn schema_str<'a>(result: &SqlResult<'a>) -> &'a str {
    result.schema.get()
}

/// Turn a decoded row into a Vista record.
pub fn row_to_record(columns: &[String], row: &ProductValue) -> Record<CborValue> {
    columns
        .iter()
        .zip(row.elements.iter())
        .map(|(name, value)| (name.clone(), algebraic_to_cbor(value)))
        .collect()
}

/// Map a SATS value onto CBOR for Vista.
///
/// Two deliberate lossy choices, both preferring a value a UI can display over
/// one that is structurally faithful but useless:
///
/// - **Integers wider than 64 bits become strings.** CBOR has no 128-bit
///   integer, and a silently truncated identifier is worse than a textual one.
/// - **`Option` unwraps**: `Some(x)` becomes `x` and `None` becomes null.
///   Nullability is a property of a Vista column, not a distinct type, so a
///   nested tagged union here would just be noise in every grid cell.
pub fn algebraic_to_cbor(value: &AlgebraicValue) -> CborValue {
    match value {
        AlgebraicValue::Bool(b) => CborValue::Bool(*b),
        AlgebraicValue::I8(v) => CborValue::Integer((*v).into()),
        AlgebraicValue::U8(v) => CborValue::Integer((*v).into()),
        AlgebraicValue::I16(v) => CborValue::Integer((*v).into()),
        AlgebraicValue::U16(v) => CborValue::Integer((*v).into()),
        AlgebraicValue::I32(v) => CborValue::Integer((*v).into()),
        AlgebraicValue::U32(v) => CborValue::Integer((*v).into()),
        AlgebraicValue::I64(v) => CborValue::Integer((*v).into()),
        AlgebraicValue::U64(v) => CborValue::Integer((*v).into()),
        AlgebraicValue::I128(v) => CborValue::Text({ v.0 }.to_string()),
        AlgebraicValue::U128(v) => CborValue::Text({ v.0 }.to_string()),
        AlgebraicValue::I256(v) => CborValue::Text(v.to_string()),
        AlgebraicValue::U256(v) => CborValue::Text(v.to_string()),
        AlgebraicValue::F32(v) => CborValue::Float(f32::from(*v) as f64),
        AlgebraicValue::F64(v) => CborValue::Float(f64::from(*v)),
        AlgebraicValue::String(s) => CborValue::Text(s.to_string()),
        AlgebraicValue::Array(array) => {
            CborValue::Array(array.iter_cloned().map(|v| algebraic_to_cbor(&v)).collect())
        }
        AlgebraicValue::Sum(sum) => {
            // `Option` is a sum of `some`/`none`; unwrap it rather than exposing
            // the tag. Any other sum keeps its tag, since dropping it would
            // discard which variant this is.
            match (sum.tag, &*sum.value) {
                (0, inner) => algebraic_to_cbor(inner),
                (1, AlgebraicValue::Product(p)) if p.elements.is_empty() => CborValue::Null,
                (tag, inner) => CborValue::Array(vec![
                    CborValue::Integer(tag.into()),
                    algebraic_to_cbor(inner),
                ]),
            }
        }
        // `Min`/`Max` are sentinel bounds used when describing index ranges,
        // not values a row can hold. If one ever reaches a record it means we
        // decoded a range as data, so null is the honest rendering.
        AlgebraicValue::Min | AlgebraicValue::Max => CborValue::Null,
        AlgebraicValue::Product(product) => {
            // Special products first: `Identity`, `ConnectionId` and the time
            // types are single-field products that mean something scalar.
            if let Some(scalar) = special_product_to_cbor(product) {
                return scalar;
            }
            CborValue::Array(product.elements.iter().map(algebraic_to_cbor).collect())
        }
    }
}

/// Render the SATS "special" products as scalars.
///
/// `Identity` and `ConnectionId` are 256-bit integers wrapped in a one-field
/// product; a hex string is what every SpacetimeDB tool prints and what a user
/// can actually match against. `Timestamp` and `TimeDuration` wrap microseconds.
fn special_product_to_cbor(product: &spacetimedb_sats::ProductValue) -> Option<CborValue> {
    let [only] = &product.elements[..] else {
        return None;
    };
    match only {
        AlgebraicValue::U256(v) => Some(CborValue::Text(format!("0x{:x}", **v))),
        AlgebraicValue::I64(micros) => Some(CborValue::Integer((*micros).into())),
        _ => None,
    }
}

/// Shorten a response body for an error message.
///
/// Cut on a character boundary rather than a byte offset. This only ever runs on
/// a body that already failed to parse, and slicing mid-character would turn
/// that error into a panic — losing the very text that explains what went wrong.
fn truncate(s: &str) -> String {
    const LIMIT: usize = 300;
    if s.len() <= LIMIT {
        return s.to_string();
    }
    let end = (0..=LIMIT)
        .rev()
        .find(|i| s.is_char_boundary(*i))
        .unwrap_or(0);
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact envelope shape a live host returns, captured from `cardroom`.
    const ENVELOPE: &str = r#"[{
        "schema": {"elements": [
            {"name": {"some": "game_id"}, "algebraic_type": {"U64": []}},
            {"name": {"some": "status"}, "algebraic_type": {"String": []}},
            {"name": {"some": "pot"}, "algebraic_type": {"I64": []}},
            {"name": {"some": "to_act_seat"}, "algebraic_type": {"Sum": {"variants": [
                {"name": {"some": "some"}, "algebraic_type": {"U64": []}},
                {"name": {"some": "none"}, "algebraic_type": {"Product": {"elements": []}}}
            ]}}}
        ]},
        "rows": [[1, "playing", 75, [0, 1]], [2, "ended", 0, [1, []]]],
        "total_duration_micros": 710,
        "stats": {"rows_inserted": 0, "rows_deleted": 0, "rows_updated": 0}
    }]"#;

    #[test]
    fn decodes_a_live_envelope_positionally() {
        let decoded = decode_sql_response(ENVELOPE).expect("envelope should decode");
        assert_eq!(decoded.columns, ["game_id", "status", "pot", "to_act_seat"]);
        assert_eq!(decoded.rows.len(), 2);
    }

    #[test]
    fn option_unwraps_to_the_value_or_null() {
        let decoded = decode_sql_response(ENVELOPE).unwrap();

        // `[0, 1]` is `Some(1)`, not a two-element array — which is exactly why
        // decoding has to be seeded with the schema.
        let first = row_to_record(&decoded.columns, &decoded.rows[0]);
        assert_eq!(first["to_act_seat"], CborValue::Integer(1.into()));

        // `[1, []]` is `None`.
        let second = row_to_record(&decoded.columns, &decoded.rows[1]);
        assert_eq!(second["to_act_seat"], CborValue::Null);
    }

    #[test]
    fn scalars_survive_the_cbor_boundary() {
        let decoded = decode_sql_response(ENVELOPE).unwrap();
        let row = row_to_record(&decoded.columns, &decoded.rows[0]);
        assert_eq!(row["game_id"], CborValue::Integer(1.into()));
        assert_eq!(row["status"], CborValue::Text("playing".into()));
        assert_eq!(row["pot"], CborValue::Integer(75.into()));
    }

    #[test]
    fn wide_integers_become_text_rather_than_truncating() {
        // u64 fits CBOR, but the 128- and 256-bit types do not. Above 2^53 is
        // also where the older JSON wire protocol would have lost precision.
        let big = AlgebraicValue::U64(9_007_199_254_740_993);
        assert_eq!(
            algebraic_to_cbor(&big),
            CborValue::Integer(9_007_199_254_740_993i64.into())
        );

        // 128-bit values have no CBOR integer to land in, so they render as
        // text rather than being truncated into one that fits.
        let huge = AlgebraicValue::U128(u128::MAX.into());
        assert_eq!(
            algebraic_to_cbor(&huge),
            CborValue::Text(u128::MAX.to_string())
        );
    }

    #[test]
    fn a_write_with_no_result_set_decodes_as_empty() {
        let decoded = decode_sql_response("[]").expect("an empty envelope is legal");
        assert!(decoded.columns.is_empty());
        assert!(decoded.rows.is_empty());
    }

    #[test]
    fn a_malformed_envelope_names_the_problem() {
        let err = decode_sql_response("not json at all").expect_err("should fail");
        assert!(
            err.to_string().contains("envelope"),
            "error should say what failed to parse: {err}"
        );
    }

    #[test]
    fn a_long_non_ascii_body_truncates_instead_of_panicking() {
        // A multi-byte character straddling the 300-byte cut. Slicing by byte
        // offset would panic here, replacing a parse error with a crash.
        let body = "é".repeat(400);
        let err = decode_sql_response(&body).expect_err("should fail to parse");
        assert!(err.to_string().contains('…'), "should be truncated: {err}");
    }
}
