//! Lowering a [`GraphqlApiVistaSpec`] onto a `Table<GraphqlApi, EmptyEntity>`:
//! the spec's `graphql:` block applied to the API, its columns typed, and its
//! `references:` registered through `Table::with_spec_references`.
//!
//! A reference target is rebuilt at traversal time from its own spec. The
//! traversal hands the build the *parent's* `GraphqlApi`, which carries the
//! parent's per-table overrides (`root_args`, `filter: false`, …). So the
//! internal resolver yields a [`TargetSpec`] that pairs the target's spec
//! with the factory's API, and the target is lowered onto that instead.

use std::sync::Arc;

use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{ColumnFlag as VistaColumnFlag, ColumnSpec, SpecResolver};

use crate::graphql::api::{GraphqlApi, Supports, split_response_path};
use crate::graphql::name::{validate, validate_path, validate_root_args};
use crate::graphql::types::AnyGraphqlType;
use crate::graphql::vista::spec::{GraphqlApiVistaSpec, GraphqlBlock, GraphqlColumnExtras};

/// Resolves a YAML spec by model name, so references can rebuild their
/// target tables from the live spec.
pub type GraphqlSpecResolver = SpecResolver<GraphqlApiVistaSpec>;

/// A reference target's spec, with the API it is lowered onto: the
/// factory's, before any table's `graphql:` block was applied.
#[derive(Clone)]
pub(crate) struct TargetSpec {
    pub(crate) spec: GraphqlApiVistaSpec,
    pub(crate) api: GraphqlApi,
}

pub(crate) type TargetResolver = SpecResolver<TargetSpec>;

/// Pair every spec `specs` resolves with `api`.
pub(crate) fn target_resolver(api: GraphqlApi, specs: GraphqlSpecResolver) -> TargetResolver {
    Arc::new(move |name: &str| {
        specs(name).map(|spec| TargetSpec {
            spec,
            api: api.clone(),
        })
    })
}

/// Lower `spec` onto `api`: block overrides, columns, id field, references.
pub(crate) fn build_table(
    spec: &GraphqlApiVistaSpec,
    api: &GraphqlApi,
    resolver: Option<TargetResolver>,
) -> Result<Table<GraphqlApi, EmptyEntity>> {
    if let Some((relation, _)) = spec.references.iter().find(|(_, r)| !r.keys.is_empty()) {
        return Err(error!(
            "GraphQL references take a single foreign_key; multi-column `keys:` are not supported",
            table = spec.name.as_str(),
            relation = relation.as_str()
        )
        .mark_unsupported());
    }

    let mut api = api.clone();
    if let Some(block) = spec.driver.graphql.as_ref() {
        apply_block(&mut api, block)?;
    }
    api.field_aliases = field_aliases(spec)?;
    let root_field = spec
        .driver
        .graphql
        .as_ref()
        .and_then(|b| b.root_field.clone())
        .unwrap_or_else(|| spec.name.clone());
    validate("root field", &root_field)?;

    let mut table = Table::<GraphqlApi, EmptyEntity>::new(root_field, api);
    table.add_spec_columns(&spec.columns, build_column)?;
    table.set_spec_id_field(&spec.resolve_id_column())?;
    table.with_spec_references(&spec.references, resolver, build_target_table)
}

/// The `with_spec_references` builder. The parent's API is ignored in favour
/// of the factory's one carried by `target` (see the module docs).
fn build_target_table(
    target: &TargetSpec,
    _parent_api: GraphqlApi,
    resolver: Option<TargetResolver>,
) -> Result<Table<GraphqlApi, EmptyEntity>> {
    build_table(&target.spec, &target.api, resolver)
}

/// Per-table dialect / filter-arg / root-field overrides from a `graphql:`
/// block onto the table's own `GraphqlApi` clone. Every name the block puts
/// into a document is checked against the GraphQL `Name` grammar here, so a
/// bad spec fails to build rather than on its first read.
fn apply_block(api: &mut GraphqlApi, block: &GraphqlBlock) -> Result<()> {
    if let Some(arg) = block.filter_arg.as_deref() {
        validate("filter argument", arg)?;
    }
    if let Some(field) = block.count_field.as_deref() {
        validate("count field", field)?;
    }
    if let Some(args) = block.args.as_ref() {
        validate_root_args(args)?;
    }
    if let Some(dialect) = block.dialect {
        api.dialect = dialect;
    }
    if let Some(arg) = block.filter_arg.clone() {
        api.filter_arg_name = Some(arg);
    }
    if let Some(args) = block.args.clone() {
        api.root_args = Some(args);
    }
    if let Some(field) = block.count_field.clone() {
        api.count_field = Some(field);
    }
    if let Some(path) = block.response_path.as_deref() {
        api.response_path = split_response_path(path);
        for segment in &api.response_path {
            validate("response path segment", segment)?;
        }
    }
    let inherited = api.supports;
    api.supports = Supports {
        filter: block.filter.or(inherited.filter),
        order: block.order.or(inherited.order),
        search: block.search.or(inherited.search),
        paginate: block.paginate.or(inherited.paginate),
        count: block.count.unwrap_or(inherited.count),
    };
    Ok(())
}

/// Column name → server field for every column with a `graphql: { field }`.
/// The alias renders as `name: field`, so both sides must be plain names.
fn field_aliases(spec: &GraphqlApiVistaSpec) -> Result<IndexMap<String, String>> {
    let mut aliases = IndexMap::new();
    for (name, column) in &spec.columns {
        let Some(field) = column
            .driver
            .graphql
            .as_ref()
            .and_then(|b| b.field.as_ref())
        else {
            continue;
        };
        if name.contains('.') || field.contains('.') {
            return Err(error!(
                "GraphQL column `field` takes plain names; dotted paths are not supported",
                column = name.as_str(),
                field = field.as_str()
            )
            .mark_unsupported());
        }
        validate("field", name)?;
        validate("field", field)?;
        aliases.insert(name.clone(), field.clone());
    }
    Ok(aliases)
}

/// The `searchable` flag carries over so quicksearch can pick its columns.
/// Only stored columns reach here (a `lazy:` one is never selected), and
/// each is a field path in the selection set, so its name is checked.
fn build_column(
    name: &str,
    col_spec: &ColumnSpec<GraphqlColumnExtras>,
) -> Result<TableColumn<AnyGraphqlType>> {
    validate_path("field", name)?;
    let column = TableColumn::from_spec(name, col_spec, None, column_for_type)?;
    Ok(if col_spec.has_flag(VistaColumnFlag::Searchable) {
        column.with_flag(ColumnFlag::Searchable)
    } else {
        column
    })
}

/// Map YAML scalar names to typed columns under `AnyGraphqlType`.
fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<AnyGraphqlType>> {
    use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
    use uuid::Uuid;

    let col: TableColumn<AnyGraphqlType> = match ty {
        "int" | "integer" | "i32" => TableColumn::from_column(TableColumn::<i32>::new(name)),
        "bigint" | "i64" | "long" => TableColumn::from_column(TableColumn::<i64>::new(name)),
        "float" | "double" | "f64" | "f32" => {
            TableColumn::from_column(TableColumn::<f64>::new(name))
        }
        "bool" | "boolean" => TableColumn::from_column(TableColumn::<bool>::new(name)),
        "string" | "text" | "str" | "id" => {
            TableColumn::from_column(TableColumn::<String>::new(name))
        }
        "datetime" | "timestamp" | "timestamptz" => {
            TableColumn::from_column(TableColumn::<DateTime<Utc>>::new(name))
        }
        "date" => TableColumn::from_column(TableColumn::<NaiveDate>::new(name)),
        "time" => TableColumn::from_column(TableColumn::<NaiveTime>::new(name)),
        "uuid" => TableColumn::from_column(TableColumn::<Uuid>::new(name)),
        "json" => TableColumn::from_column(TableColumn::<serde_json::Value>::new(name)),
        other => {
            return Err(error!(
                "Unknown YAML column type for GraphQL Vista",
                column = name.to_string(),
                ty = other.to_string()
            ));
        }
    };
    Ok(col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphql::api::CountMode;

    fn lower(api: &GraphqlApi, yaml: &str) -> GraphqlApi {
        let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        build_table(&spec, api, None).unwrap().data_source().clone()
    }

    #[test]
    fn block_without_supports_keys_keeps_the_builders() {
        let api = GraphqlApi::builder("https://api.test/graphql")
            .supports(
                Supports::default()
                    .with_paginate(true)
                    .with_count(CountMode::Aggregate),
            )
            .build();
        let lowered = lower(
            &api,
            "name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\ngraphql:\n  root_field: launchesPast\n",
        );
        assert!(lowered.can_paginate());
        assert_eq!(lowered.supports.count, CountMode::Aggregate);
    }

    #[test]
    fn block_supports_keys_override_the_builders() {
        let api = GraphqlApi::builder("https://api.test/graphql")
            .supports(Supports::default().with_paginate(true))
            .build();
        let lowered = lower(
            &api,
            "name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\ngraphql:\n  paginate: false\n",
        );
        assert!(!lowered.can_paginate());
    }
}
