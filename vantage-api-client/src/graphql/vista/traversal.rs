//! Reference traversal on a `GraphqlApiTableShell`.
//!
//! The typed table holds every reference as `with_one` / `with_many`, which
//! builds the bare target. A spec-built shell builds it from the target's
//! spec instead: the spec names the traversed Vista, carries the target's
//! own references into its metadata, and is checked up front, since
//! vantage-table turns a target it cannot build into a column-less table
//! rather than an error. From a parent row, the target is narrowed through
//! `Reference::narrow`, which passes the parent's values unchanged; the
//! target's `add_condition` types each by its column.

use ciborium::Value as CborValue;
use vantage_core::{Context, Result, error};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Vista, unknown_relation};

use crate::graphql::api::GraphqlApi;
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
            .mark_not_found()
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

    /// The target of `relation`: bare without a `row`, else narrowed to the
    /// rows related to it through the reference's join keys.
    pub(crate) fn ref_target(
        &self,
        relation: &str,
        row: Option<&Record<CborValue>>,
    ) -> Result<Vista> {
        let reference = self
            .metadata
            .references
            .get(relation)
            .ok_or_else(|| unknown_relation(relation))?;
        let (target_spec, bare) = match self.spec_target(relation)? {
            Some((spec, bare)) => (Some(spec), bare),
            None => (None, self.table.get_ref_target_erased(relation)?),
        };
        let target = self.wrap_target(bare, target_spec)?;
        match row {
            Some(row) => reference.narrow(target, row),
            None => Ok(target),
        }
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
                wrap_spec_table(table, &target.spec, resolver.clone())
            }
            _ => GraphqlApiVistaFactory::new(self.table.data_source().clone()).from_table(table),
        }
    }
}
