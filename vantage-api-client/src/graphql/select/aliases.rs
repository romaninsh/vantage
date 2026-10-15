//! Column names versus server field names.
//!
//! A column can read a server field of another name (`graphql: { field: … }`
//! in YAML). The rest of the crate speaks column names only; the select
//! translates on the way out: the selection set asks for `name: field` (a
//! GraphQL alias, so the response arrives keyed by the column name) and
//! condition and sort keys are renamed to the server field.

use indexmap::IndexMap;
use vantage_expressions::Order;

use crate::graphql::condition::{FieldCondition, GraphqlCondition};
use crate::graphql::select::GraphqlSelect;

impl GraphqlSelect {
    /// The server field a column reads: its alias target, else the column
    /// name itself.
    fn wire_name<'a>(&'a self, column: &'a str) -> &'a str {
        self.aliases.get(column).map_or(column, String::as_str)
    }

    /// The selected paths as they go into the selection set, with every
    /// aliased column spelled `name: field`.
    pub(crate) fn selection_paths(&self) -> Vec<String> {
        self.fields
            .iter()
            .map(|path| match self.aliases.get(path) {
                Some(field) => format!("{path}: {field}"),
                None => path.clone(),
            })
            .collect()
    }

    /// The conditions with their field keys renamed to server fields.
    /// A [`GraphqlCondition::Deferred`] object is opaque and goes out as is.
    pub(crate) fn wire_conditions(&self) -> Vec<GraphqlCondition> {
        self.conditions
            .iter()
            .map(|c| rename(c, &self.aliases))
            .collect()
    }

    /// The sort keys renamed to server fields.
    pub(crate) fn wire_sort(&self) -> Vec<(&str, Order)> {
        self.sort
            .iter()
            .map(|(field, order)| (self.wire_name(field), *order))
            .collect()
    }
}

fn rename(condition: &GraphqlCondition, aliases: &IndexMap<String, String>) -> GraphqlCondition {
    if aliases.is_empty() {
        return condition.clone();
    }
    let wire = |field: &String| aliases.get(field).unwrap_or(field).clone();
    match condition {
        GraphqlCondition::Field(fc) => GraphqlCondition::Field(FieldCondition::new(
            wire(&fc.field),
            fc.op.clone(),
            fc.value.clone(),
        )),
        GraphqlCondition::DeferredField {
            field,
            op,
            value_fn,
        } => GraphqlCondition::deferred_field(wire(field), op.clone(), value_fn.clone()),
        GraphqlCondition::And(parts) => {
            GraphqlCondition::And(parts.iter().map(|p| rename(p, aliases)).collect())
        }
        GraphqlCondition::Or(parts) => {
            GraphqlCondition::Or(parts.iter().map(|p| rename(p, aliases)).collect())
        }
        GraphqlCondition::Not(inner) => GraphqlCondition::Not(Box::new(rename(inner, aliases))),
        GraphqlCondition::Deferred(_) => condition.clone(),
    }
}
