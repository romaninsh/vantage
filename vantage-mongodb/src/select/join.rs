//! Joined columns: a dotted column (`client.name`) read off the document a
//! chain of `has_one` references leads to, lowered to `$lookup` stages.
//!
//! Each distinct hop chain is looked up once into a temporary field
//! (`__vj<k>`); every joined column is then computed into its own alias
//! (`__vc<i>`, `i` its position in `MongoSelect::joins`) and the temporaries
//! are dropped. A dangling reference leaves the alias missing, so the row
//! still reads.
//!
//! `$lookup` matches by strict BSON equality: an `ObjectId` reference joins
//! an `ObjectId` `_id`, but a hex string does not. It also treats a missing
//! local field as `null`, so a row without a reference joins the target
//! documents whose foreign field is null or missing; against `_id` (never
//! missing) that matches nothing.

use bson::{Bson, Document, doc};

use super::MongoSelect;

/// One `has_one` hop: the documents of `from` whose `foreign_field` equals
/// `local_field`, a BSON path in the document doing the joining.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JoinHop {
    pub from: String,
    pub local_field: String,
    pub foreign_field: String,
}

/// A read-only column named `name`: the value at `field` on the document
/// reached through `hops`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JoinColumn {
    pub name: String,
    pub hops: Vec<JoinHop>,
    pub field: String,
}

const ALIAS_PREFIX: &str = "__vc";

/// The field the `i`th joined column is computed into.
pub(crate) fn join_alias(i: usize) -> String {
    format!("{ALIAS_PREFIX}{i}")
}

/// The join index `field` is the alias of, if it is one.
pub(crate) fn alias_index(field: &str) -> Option<usize> {
    let digits = field.strip_prefix(ALIAS_PREFIX)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn temp_field(k: usize) -> String {
    format!("__vj{k}")
}

/// Whether the field path `path` starts at one of the first `count` join
/// aliases.
fn names_alias(path: &str, count: usize) -> bool {
    let head = path.split('.').next().unwrap_or(path);
    alias_index(head).is_some_and(|i| i < count)
}

/// Whether `filter` names any of the first `count` join aliases, as a key or
/// as a `$`-prefixed field reference, at any depth.
pub(crate) fn filter_uses_joins(filter: &Document, count: usize) -> bool {
    count > 0
        && filter
            .iter()
            .any(|(k, v)| names_alias(k, count) || mentions_joins(v, count))
}

fn mentions_joins(value: &Bson, count: usize) -> bool {
    match value {
        Bson::Document(d) => filter_uses_joins(d, count),
        Bson::Array(items) => items.iter().any(|v| mentions_joins(v, count)),
        Bson::String(s) => s.strip_prefix('$').is_some_and(|p| names_alias(p, count)),
        _ => false,
    }
}

impl MongoSelect {
    /// The stages that compute every joined column, empty without joins:
    /// a `$lookup` + `$set` pair per distinct hop chain, one `$addFields`
    /// with the aliases, then `$unset` of the temporaries.
    pub(crate) fn join_stages(&self) -> Vec<Document> {
        if self.joins.is_empty() {
            return Vec::new();
        }
        let mut stages = Vec::new();
        let mut chains: Vec<&[JoinHop]> = Vec::new();
        let mut values = Document::new();
        for (i, column) in self.joins.iter().enumerate() {
            let mut parent: Option<usize> = None;
            for depth in 1..=column.hops.len() {
                let chain = &column.hops[..depth];
                let k = match chains.iter().position(|c| *c == chain) {
                    Some(k) => k,
                    None => {
                        let hop = &chain[depth - 1];
                        let local = match parent {
                            Some(p) => format!("{}.{}", temp_field(p), hop.local_field),
                            None => hop.local_field.clone(),
                        };
                        let temp = temp_field(chains.len());
                        stages.push(doc! { "$lookup": {
                            "from": &hop.from,
                            "localField": local,
                            "foreignField": &hop.foreign_field,
                            "as": &temp,
                        } });
                        stages.push(doc! { "$set": { &temp: { "$first": format!("${temp}") } } });
                        chains.push(chain);
                        chains.len() - 1
                    }
                };
                parent = Some(k);
            }
            if let Some(k) = parent {
                values.insert(
                    join_alias(i),
                    format!("${}.{}", temp_field(k), column.field),
                );
            }
        }
        stages.push(doc! { "$addFields": values });
        let temps: Vec<String> = (0..chains.len()).map(temp_field).collect();
        stages.push(doc! { "$unset": temps });
        stages
    }

    /// Whether the sort orders by a joined column.
    pub(crate) fn sort_uses_joins(&self) -> bool {
        self.sort
            .iter()
            .any(|(field, _)| names_alias(field, self.joins.len()))
    }
}
