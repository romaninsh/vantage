//! Offset paging for `GraphqlApiTableShell`.
//!
//! Every paged read runs on a clone of the wrapped table with a
//! `Pagination` set, so the shell's own query state never carries a page.
//! The select builder turns that into `limit:`/`offset:` — only when the
//! table's root field accepts them (`paginate: true`).
//!
//! Offset paging is only stable under a deterministic order. Where the
//! dialect can render `order_by` (Hasura), a paged read with no consumer
//! order is ordered by the id column. Generic schemas have no `order_by`
//! spelling, so page stability there depends on the API returning rows in
//! a consistent natural order.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::column::core::Column;
use vantage_table::pagination::Pagination;
use vantage_table::sorting::{OrderBy, SortDirection};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use crate::graphql::api::GraphqlApi;
use crate::graphql::operation::GraphqlOperation;
use crate::graphql::types::AnyGraphqlType;
use crate::graphql::vista::source::GraphqlApiTableShell;

impl GraphqlApiTableShell {
    /// The wrapped table, ordered by its id column when the dialect can
    /// order and the consumer set no order of their own.
    ///
    /// A dotted id (`run.id`) is left alone: Hasura's `order_by` spells a
    /// nested field as a nested object, which the renderer doesn't produce.
    fn ordered_for_paging(&self) -> Table<GraphqlApi, EmptyEntity> {
        let mut table = self.table.clone();
        if !table.data_source().can_order() || table.orders().next().is_some() {
            return table;
        }
        let Some(id) = table.id_field().map(|c| c.name().to_string()) else {
            return table;
        };
        if id.contains('.') {
            return table;
        }
        // Same carrier as `add_order`: the renderer reads the column name
        // out of an eq-condition's field; the value is never rendered.
        table.add_order(OrderBy {
            expression: Column::<AnyGraphqlType>::new(&id).eq(id.as_str()),
            direction: SortDirection::Ascending,
        });
        table
    }

    /// List one page or window of rows as CBOR records, in server order.
    pub(crate) async fn list_paginated(
        &self,
        pagination: Pagination,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        let mut table = self.ordered_for_paging();
        table.set_pagination(Some(pagination));
        let raw = table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(id, record)| (id, Self::record_to_cbor(record)))
            .collect())
    }

    /// Page size from `set_page_size`, required before `fetch_page` and
    /// `fetch_next`.
    pub(crate) fn require_page_size(&self, method: &str) -> Result<usize> {
        self.page_size.ok_or_else(|| {
            error!(
                "set_page_size must be called before fetching pages",
                method = method
            )
        })
    }

    /// One `fetch_next` step. The token is the 1-based page number of the
    /// next fetch (`None` is page 1); a short page — including an empty
    /// one — ends the chain.
    pub(crate) async fn fetch_next_page(
        &self,
        token: Option<CborValue>,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<CborValue>)> {
        let size = self.require_page_size("fetch_next")?;
        let page: i64 = match token {
            None => 1,
            Some(CborValue::Integer(n)) => {
                i64::try_from(n).map_err(|_| error!("fetch_next token out of i64 range"))?
            }
            Some(_) => return Err(error!("invalid fetch_next token type for graphql driver")),
        };
        if page < 1 {
            return Err(error!(
                "fetch_next token must be a 1-based page number",
                page = page
            ));
        }
        let records = self
            .list_paginated(Pagination::new(page, size as i64))
            .await?;
        let next = (records.len() == size).then(|| CborValue::Integer((page + 1).into()));
        Ok((records, next))
    }
}
