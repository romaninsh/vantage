//! The names a `GraphqlSelect` splices into its document, checked against
//! the GraphQL `Name` grammar before anything is rendered.

use serde_json::Value;
use vantage_core::Result;

use crate::graphql::condition::FilterDialect;
use crate::graphql::name::{validate, validate_object_keys, validate_path, validate_root_args};
use crate::graphql::select::GraphqlSelect;
use crate::graphql::select::render::json_to_graphql_value;

impl GraphqlSelect {
    /// The filter argument's name: the override, else `where` (Hasura) or
    /// `find` (Generic).
    pub(crate) fn filter_arg(&self) -> &str {
        self.filter_arg_name
            .as_deref()
            .unwrap_or(match self.dialect {
                FilterDialect::Hasura => "where",
                FilterDialect::Generic => "find",
            })
    }

    /// Check the names in the selection: operation name, fields (dotted
    /// paths segment by segment), column aliases, response envelope, order
    /// fields, and the same for every sub-selection.
    pub(crate) fn validate_selection_names(&self) -> Result<()> {
        if let Some(op) = self.operation_name.as_deref().filter(|n| !n.is_empty()) {
            validate("operation name", op)?;
        }
        for field in &self.fields {
            validate_path("field", field)?;
        }
        for (column, field) in &self.aliases {
            validate("field", column)?;
            validate("field", field)?;
        }
        for segment in &self.response_path {
            validate("response path segment", segment)?;
        }
        for (field, _) in &self.sort {
            validate("order field", field)?;
        }
        for (field, child) in &self.sub_selections {
            validate("field", field)?;
            child.validate_selection_names()?;
        }
        Ok(())
    }

    /// `name: value` for each literal root argument, keys checked at every
    /// depth.
    pub(crate) fn render_root_args(&self) -> Result<Vec<String>> {
        let Some(args @ Value::Object(map)) = &self.root_args else {
            return Ok(Vec::new());
        };
        validate_root_args(args)?;
        Ok(map
            .iter()
            .map(|(name, value)| format!("{}: {}", name, json_to_graphql_value(value)))
            .collect())
    }

    /// `<filter arg>: <rendered condition>`, the argument name and every
    /// key of the condition object checked.
    pub(crate) fn render_filter_arg(&self, condition: &Value) -> Result<String> {
        let arg = self.filter_arg();
        validate("filter argument", arg)?;
        validate_object_keys("filter key", condition)?;
        Ok(format!("{}: {}", arg, json_to_graphql_value(condition)))
    }
}
