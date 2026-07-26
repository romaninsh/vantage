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
use spacetimedb_sats::{AlgebraicType, AlgebraicValue, ProductType, ProductValue, Typespace};
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

/// Decoded rows plus the schema they line up with.
#[derive(Debug)]
pub struct DecodedRows {
    pub columns: Vec<String>,
    pub rows: Vec<ProductValue>,
    /// The row's declared type, kept rather than discarded after decoding.
    ///
    /// A SATS *value* does not say what it means: a sum carries a variant index
    /// and nothing about whether its type is an `Option`, and `Identity` is a
    /// one-field product indistinguishable by shape from any other one-field
    /// product. Rendering therefore needs the type, and the response already
    /// carries it — it is what seeds the decode two dozen lines below.
    pub schema: ProductType,
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
            schema: ProductType::new([].into()),
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

    Ok(DecodedRows {
        columns,
        rows,
        schema,
    })
}

fn schema_str<'a>(result: &SqlResult<'a>) -> &'a str {
    result.schema.get()
}

/// Turn a decoded row into a Vista record.
///
/// Takes the row's type as well as its values, because several SATS shapes mean
/// nothing without it — see [`algebraic_to_cbor`].
pub fn row_to_record(
    columns: &[String],
    schema: &ProductType,
    row: &ProductValue,
) -> Record<CborValue> {
    columns
        .iter()
        .zip(row.elements.iter())
        .enumerate()
        .map(|(i, (name, value))| {
            let ty = schema.elements.get(i).map(|e| &e.algebraic_type);
            (name.clone(), algebraic_to_cbor(ty, value))
        })
        .collect()
}

/// Map a SATS value onto CBOR for Vista, guided by its declared type.
///
/// Two deliberate lossy choices, both preferring a value a UI can display over
/// one that is structurally faithful but useless:
///
/// - **Integers wider than 64 bits become strings.** CBOR has no 128-bit
///   integer, and a silently truncated identifier is worse than a textual one.
/// - **`Option` unwraps**: `Some(x)` becomes `x` and `None` becomes null.
///   Nullability is a property of a Vista column, not a distinct type, so a
///   nested tagged union here would just be noise in every grid cell.
///
/// # Why the type has to come along
///
/// A SATS value does not describe itself. A sum carries a variant *index* and,
/// per the spec, nothing more — "a non-negative integer… an index into the
/// `SumType.variants` array". Reading `Option` out of that index alone means
/// assuming tag 0 is always `Some` and tag 1 always `None`, which holds for
/// `Option` and for nothing else: a three-variant `Status` enum would then have
/// its first variant silently stripped of its tag, its second rendered as
/// `null`, and its third left tagged — three encodings for one column.
///
/// Products have the same problem in reverse. `Identity` is a single `U256` in a
/// one-field product, and `Timestamp` a single `I64`; deciding from that shape
/// alone turns any user struct with one such field into a hex string or a bare
/// integer.
///
/// So `ty` decides, and only falls back to the shape when it is absent —
/// `None` here means a projection whose type the response did not describe,
/// where a tagged rendering is the honest answer rather than a guessed one.
pub fn algebraic_to_cbor(ty: Option<&AlgebraicType>, value: &AlgebraicValue) -> CborValue {
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
            let element = match ty {
                Some(AlgebraicType::Array(a)) => Some(&*a.elem_ty),
                _ => None,
            };
            CborValue::Array(
                array
                    .iter_cloned()
                    .map(|v| algebraic_to_cbor(element, &v))
                    .collect(),
            )
        }
        AlgebraicValue::Sum(sum) => {
            let sum_ty = match ty {
                Some(AlgebraicType::Sum(s)) => Some(s),
                _ => None,
            };
            sum_to_cbor(sum_ty, sum)
        }
        // `Min`/`Max` are sentinel bounds used when describing index ranges,
        // not values a row can hold. If one ever reaches a record it means we
        // decoded a range as data, so null is the honest rendering.
        AlgebraicValue::Min | AlgebraicValue::Max => CborValue::Null,
        AlgebraicValue::Product(product) => {
            // Special products first: `Identity`, `ConnectionId` and the time
            // types are single-field products that mean something scalar — but
            // only when the *type* says so.
            if let Some(scalar) = special_product_to_cbor(ty, product) {
                return scalar;
            }
            let fields = match ty {
                Some(AlgebraicType::Product(p)) => Some(p),
                _ => None,
            };
            CborValue::Array(
                product
                    .elements
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        algebraic_to_cbor(
                            fields
                                .and_then(|p| p.elements.get(i))
                                .map(|e| &e.algebraic_type),
                            v,
                        )
                    })
                    .collect(),
            )
        }
    }
}

/// Render a sum, using its type to tell an `Option` from an ordinary enum.
///
/// A named variant renders as its name, which is what a grid can show and a
/// filter can match; an unnamed one falls back to its index. Either way every
/// variant of a given column renders the same way as its siblings — the
/// property that index-guessing could not provide.
fn sum_to_cbor(
    ty: Option<&spacetimedb_sats::SumType>,
    sum: &spacetimedb_sats::SumValue,
) -> CborValue {
    let Some(sum_ty) = ty else {
        // No type: tag it and keep everything, rather than guess which of the
        // two tags might have meant `None`.
        return CborValue::Array(vec![
            CborValue::Integer(sum.tag.into()),
            algebraic_to_cbor(None, &sum.value),
        ]);
    };

    if let Some(some_ty) = sum_ty.as_option() {
        // Tag 0 is `some`, tag 1 is `none` — true *because the type is an
        // `Option`*, not because of the tag values themselves.
        return match sum.tag {
            0 => algebraic_to_cbor(Some(some_ty), &sum.value),
            _ => CborValue::Null,
        };
    }

    let variant = sum_ty.variants.get(sum.tag as usize);
    let label = variant
        .and_then(|v| v.name())
        .map(|n| n.to_string())
        .unwrap_or_else(|| sum.tag.to_string());
    let payload_ty = variant.map(|v| &v.algebraic_type);

    // A unit variant is its name; there is no payload worth an array around it.
    if matches!(&*sum.value, AlgebraicValue::Product(p) if p.elements.is_empty()) {
        return CborValue::Text(label);
    }
    CborValue::Array(vec![
        CborValue::Text(label),
        algebraic_to_cbor(payload_ty, &sum.value),
    ])
}

/// Render the SATS "special" products as scalars.
///
/// `Identity` and `ConnectionId` are 256-bit integers wrapped in a one-field
/// product; a hex string is what every SpacetimeDB tool prints and what a user
/// can actually match against. `Timestamp` and `TimeDuration` wrap microseconds.
///
/// Recognised from the declared type rather than the value's shape. By shape,
/// every one-field product holding a `U256` looks like an `Identity`, so a user
/// struct that happened to have one would be rendered as a hex string and never
/// show its field.
fn special_product_to_cbor(
    ty: Option<&AlgebraicType>,
    product: &spacetimedb_sats::ProductValue,
) -> Option<CborValue> {
    let ty = ty?;
    let [only] = &product.elements[..] else {
        return None;
    };
    if (ty.is_identity() || ty.is_connection_id())
        && let AlgebraicValue::U256(v) = only
    {
        return Some(CborValue::Text(format!("0x{:x}", **v)));
    }
    if (ty.is_timestamp() || ty.is_time_duration())
        && let AlgebraicValue::I64(micros) = only
    {
        return Some(CborValue::Integer((*micros).into()));
    }
    None
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
        let first = row_to_record(&decoded.columns, &decoded.schema, &decoded.rows[0]);
        assert_eq!(first["to_act_seat"], CborValue::Integer(1.into()));

        // `[1, []]` is `None`.
        let second = row_to_record(&decoded.columns, &decoded.schema, &decoded.rows[1]);
        assert_eq!(second["to_act_seat"], CborValue::Null);
    }

    #[test]
    fn scalars_survive_the_cbor_boundary() {
        let decoded = decode_sql_response(ENVELOPE).unwrap();
        let row = row_to_record(&decoded.columns, &decoded.schema, &decoded.rows[0]);
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
            algebraic_to_cbor(None, &big),
            CborValue::Integer(9_007_199_254_740_993i64.into())
        );

        // 128-bit values have no CBOR integer to land in, so they render as
        // text rather than being truncated into one that fits.
        let huge = AlgebraicValue::U128(u128::MAX.into());
        assert_eq!(
            algebraic_to_cbor(None, &huge),
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

    /// A three-variant unit enum — an ordinary SpacetimeDB idiom, and the shape
    /// that index-guessing mangled.
    fn status_enum() -> AlgebraicType {
        AlgebraicType::sum([
            ("Active", AlgebraicType::unit()),
            ("Idle", AlgebraicType::unit()),
            ("Busy", AlgebraicType::unit()),
        ])
    }

    fn variant(tag: u8) -> AlgebraicValue {
        AlgebraicValue::Sum(spacetimedb_sats::SumValue {
            tag,
            value: Box::new(AlgebraicValue::Product(
                std::iter::empty::<AlgebraicValue>().collect(),
            )),
        })
    }

    #[test]
    fn every_variant_of_an_enum_renders_the_same_way() {
        // Judging by tag alone gave one column three encodings: variant 0 lost
        // its tag entirely, variant 1 became `null` — indistinguishable from a
        // missing value — and variant 2 stayed a tagged array. With the type in
        // hand each is simply its own name.
        let ty = status_enum();
        assert_eq!(
            algebraic_to_cbor(Some(&ty), &variant(0)),
            CborValue::Text("Active".into())
        );
        assert_eq!(
            algebraic_to_cbor(Some(&ty), &variant(1)),
            CborValue::Text("Idle".into())
        );
        assert_eq!(
            algebraic_to_cbor(Some(&ty), &variant(2)),
            CborValue::Text("Busy".into())
        );
    }

    #[test]
    fn an_option_still_unwraps() {
        // The behaviour the tag-guessing was reaching for, now held for the right
        // reason: the *type* is an `Option`, not the tag happening to be 0 or 1.
        let ty = AlgebraicType::option(AlgebraicType::U64);
        let some = AlgebraicValue::Sum(spacetimedb_sats::SumValue {
            tag: 0,
            value: Box::new(AlgebraicValue::U64(7)),
        });
        assert_eq!(
            algebraic_to_cbor(Some(&ty), &some),
            CborValue::Integer(7.into())
        );
        assert_eq!(algebraic_to_cbor(Some(&ty), &variant(1)), CborValue::Null);
    }

    #[test]
    fn a_one_field_user_struct_is_not_mistaken_for_an_identity() {
        // By shape this is an `Identity`: a one-field product holding a u256.
        // Only the declared type separates them, and getting it wrong would show
        // a hex string in place of the field.
        let ty = AlgebraicType::product([("big", AlgebraicType::U256)]);
        let value = AlgebraicValue::Product(
            std::iter::once(AlgebraicValue::U256(Box::new(
                spacetimedb_sats::u256::from(255u64),
            )))
            .collect(),
        );
        assert_eq!(
            algebraic_to_cbor(Some(&ty), &value),
            CborValue::Array(vec![CborValue::Text("255".into())]),
            "a plain struct should keep its field, not collapse to an identity"
        );
    }
}
