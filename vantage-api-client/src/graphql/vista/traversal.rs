//! Reference traversal on a `GraphqlApiTableShell`.
//!
//! The typed table holds every reference as `with_one` / `with_many`, so the
//! eq-condition on the target comes from vantage-table either way. What a
//! spec-built shell adds is the target's spec: it names the traversed Vista,
//! carries the target's own references into its metadata, and is checked
//! up front, since vantage-table turns a target it cannot build into a
//! column-less table rather than an error. The join value read from the
//! parent row is retyped by the target column first (`coerce_join_value`).

use ciborium::Value as CborValue;
use vantage_core::{Context, Result, error};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{ReferenceKind, Vista};

use crate::graphql::api::GraphqlApi;
use crate::graphql::impls::get_by_id::id_for_column;
use crate::graphql::types::AnyGraphqlType;
use crate::graphql::vista::factory::{GraphqlApiVistaFactory, wrap_spec_table};
use crate::graphql::vista::lowering::{TargetSpec, build_table};
use crate::graphql::vista::source::GraphqlApiTableShell;

pub(crate) type BareTarget = Table<GraphqlApi, EmptyEntity>;

impl GraphqlApiTableShell {
    /// The target spec of a YAML `references:` entry with its bare (not yet
    /// narrowed) table, or `None` for a shell wrapped from a typed table.
    /// Errors when the resolver does not know the target or its spec does
    /// not lower.
    pub(crate) fn spec_target(&self, relation: &str) -> Result<Option<(TargetSpec, BareTarget)>> {
        let (Some(resolver), Some(reference)) =
            (&self.resolver, self.metadata.references.get(relation))
        else {
            return Ok(None);
        };
        let target_name = reference.target.as_str();
        let target = resolver(target_name).ok_or_else(|| {
            error!(
                "GraphQL reference target has no registered spec; register it with \
                 `register_yaml` or resolve it through `with_resolver`",
                relation = relation,
                target = target_name
            )
        })?;
        let bare =
            build_table(&target.spec, &target.api, Some(resolver.clone())).with_context(|| {
                error!(
                    "GraphQL reference target spec can't be built",
                    relation = relation,
                    target = target_name
                )
            })?;
        Ok(Some((target, bare)))
    }

    /// Retype a text join value in `native` (the converted `row`) by the
    /// column of `target` (the bare target table) it is compared with: an
    /// integer-typed column takes an integer literal. Rows built from text
    /// (forms, scripts, URLs) would otherwise send `_eq: "5"` against an
    /// `Int` field, which the server rejects.
    pub(crate) fn coerce_join_value(
        &self,
        relation: &str,
        target: &BareTarget,
        row: &Record<CborValue>,
        native: &mut Record<AnyGraphqlType>,
    ) -> Result<()> {
        let Some(reference) = self
            .table
            .vista_references()
            .into_iter()
            .find(|r| r.name == relation)
        else {
            return Ok(());
        };
        let (source, target_column) = match reference.kind {
            ReferenceKind::HasOne => (reference.foreign_key, target.id_field()),
            ReferenceKind::HasMany => {
                let column = target.columns().get(&reference.foreign_key);
                (self.table.id_field_name(), column)
            }
        };
        if let Some(CborValue::Text(text)) = row.get(&source) {
            let value = id_for_column(target_column, text, &source)?;
            native.insert(source, AnyGraphqlType::untyped(value));
        }
        Ok(())
    }

    /// Wrap a traversed table: from its spec when it has one, else as a
    /// typed table on the same data source.
    pub(crate) fn wrap_target(
        &self,
        table: Table<GraphqlApi, EmptyEntity>,
        target: Option<TargetSpec>,
    ) -> Result<Vista> {
        match (target, &self.resolver) {
            (Some(target), Some(resolver)) => {
                Ok(wrap_spec_table(table, &target.spec, resolver.clone()))
            }
            _ => GraphqlApiVistaFactory::new(self.table.data_source().clone()).from_table(table),
        }
    }
}
