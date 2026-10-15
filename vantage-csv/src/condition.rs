//! Evaluates `Expression<AnyCsvType>` conditions against parsed rows.
//! Supports `eq` and `in`; search and other operators are refused.

use indexmap::IndexMap;
use vantage_core::Result;
use vantage_expressions::Expression;
use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_types::Record;

use crate::operation::{OP_EQ, OP_IN, OP_SEARCH};
use crate::type_system::AnyCsvType;

/// Evaluate a single condition expression against a set of records,
/// returning only the records that match. `Deferred` params are resolved first.
pub(crate) async fn apply_condition(
    records: IndexMap<String, Record<AnyCsvType>>,
    condition: &Expression<AnyCsvType>,
) -> Result<IndexMap<String, Record<AnyCsvType>>> {
    if condition.template == OP_SEARCH {
        return Err(vantage_core::error!(
            "CSV does not support full-table search; load into memory (Lens/Diorama) to search"
        )
        .mark_unsupported()
        .traced());
    }

    let params = &condition.parameters;
    if params.len() < 2 {
        return Ok(records);
    }

    let field_name = match &params[0] {
        ExpressiveEnum::Nested(expr) => expr.template.clone(),
        _ => return Ok(records),
    };

    match condition.template.as_str() {
        OP_EQ => {
            let expected = resolve_param(&params[1]).await?;
            Ok(records
                .into_iter()
                .filter(|(_id, record)| {
                    record
                        .get(&field_name)
                        .map(|v| v.value() == expected.value())
                        .unwrap_or(false)
                })
                .collect())
        }
        OP_IN => {
            let resolved = resolve_param(&params[1]).await?;
            let match_values: Vec<AnyCsvType> = resolved
                .try_get::<Vec<AnyCsvType>>()
                .unwrap_or_else(|| vec![resolved.clone()]);
            Ok(records
                .into_iter()
                .filter(|(_id, record)| {
                    record
                        .get(&field_name)
                        .map(|v| match_values.iter().any(|m| m.value() == v.value()))
                        .unwrap_or(false)
                })
                .collect())
        }
        other => Err(vantage_core::error!(
            "Unsupported CSV condition operator",
            template = other.to_string()
        )),
    }
}

/// Resolve an expression parameter to a concrete `AnyCsvType` value.
/// A nested expression with no single parameter is taken as its template text.
pub(crate) async fn resolve_param(param: &ExpressiveEnum<AnyCsvType>) -> Result<AnyCsvType> {
    vantage_expressions::resolve_param(param, |template| AnyCsvType::new(template.to_string()))
        .await
}
