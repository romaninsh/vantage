//! Keyset paging for S3 object listings (`ListObjectsV2`).
//!
//! The token is the **last key of the previous page**, sent as
//! `start-after`: S3 lists keys in lexicographic order and accepts any key
//! as a starting point, so unlike an opaque continuation token this cursor
//! survives process restarts and can be rebuilt from fetched rows. The page
//! size is sent as `max-keys`, and `IsTruncated` on the response decides
//! whether a next page exists.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_vista::Page;

use super::AwsTableShell;
use crate::condition::AwsCondition;
use crate::impls::table_source::retain_eq_conditions;

/// Whether the operation behind `table_name` takes `start-after` and
/// `max-keys`, so it can page by key.
pub(crate) fn pages_by_key(table_name: &str) -> bool {
    crate::dispatch::parse_table_name(table_name).is_ok_and(|op| {
        op.protocol == crate::dispatch::Protocol::RestXml && op.target.contains("list-type=2")
    })
}

impl AwsTableShell {
    /// Up to `limit` rows after the key `token`, and the last key returned
    /// when the listing has more.
    pub(super) async fn fetch_after(
        &self,
        token: Option<CborValue>,
        limit: usize,
    ) -> Result<(Page, Option<CborValue>)> {
        let account = self.table.data_source();
        let name = self.table.table_name();
        let conditions: Vec<AwsCondition> = self.table.conditions().cloned().collect();
        let request = page_request(&conditions, token, limit)?;
        let resp = account.execute_rpc_page(name, &request).await?;
        let truncated = match resp.get("IsTruncated") {
            Some(serde_json::Value::String(s)) => s == "true",
            Some(serde_json::Value::Bool(b)) => *b,
            _ => false,
        };
        let mut rows = account.parse_records(name, resp, self.metadata.id_column.as_deref())?;
        // The next page starts after the last key the API returned, before
        // record-field conditions drop any rows from this one.
        let next = truncated
            .then(|| rows.keys().last().map(|k| CborValue::Text(k.clone())))
            .flatten();
        retain_eq_conditions(&mut rows, &conditions, &self.table.row_fields());
        Ok((Page::new(rows.into_iter().collect()), next))
    }
}

/// The set's `conditions` plus the paging parameters for one page: the page
/// size as `max-keys` and the token as `start-after`.
pub(super) fn page_request(
    conditions: &[AwsCondition],
    token: Option<CborValue>,
    limit: usize,
) -> Result<Vec<AwsCondition>> {
    if limit == 0 {
        return Err(error!("cursor limit must be positive"));
    }
    let mut request = conditions.to_vec();
    request.push(AwsCondition::eq("max-keys", limit.to_string()));
    match token {
        Some(CborValue::Text(after)) => request.push(AwsCondition::eq("start-after", after)),
        Some(other) => {
            return Err(error!(
                "S3 cursor token must be the text key of the last row",
                token = format!("{other:?}")
            ));
        }
        None => {}
    }
    Ok(request)
}
