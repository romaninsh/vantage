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

#[cfg(test)]
mod tests {
    use super::*;

    fn hop(from: &str, local: &str) -> JoinHop {
        JoinHop {
            from: from.into(),
            local_field: local.into(),
            foreign_field: "_id".into(),
        }
    }

    fn column(name: &str, hops: Vec<JoinHop>, field: &str) -> JoinColumn {
        JoinColumn {
            name: name.into(),
            hops,
            field: field.into(),
        }
    }

    #[test]
    fn one_hop_join_renders_lookup_alias_and_cleanup() {
        let mut s = MongoSelect::new();
        s.joins = vec![column(
            "client.name",
            vec![hop("clients", "client_id")],
            "name",
        )];
        assert_eq!(
            s.join_stages(),
            vec![
                doc! { "$lookup": { "from": "clients", "localField": "client_id", "foreignField": "_id", "as": "__vj0" } },
                doc! { "$set": { "__vj0": { "$first": "$__vj0" } } },
                doc! { "$addFields": { "__vc0": "$__vj0.name" } },
                doc! { "$unset": ["__vj0"] },
            ]
        );
    }

    #[test]
    fn two_hop_join_chains_through_the_first_lookup() {
        let mut s = MongoSelect::new();
        s.joins = vec![column(
            "client.country.name",
            vec![hop("clients", "client_id"), hop("countries", "country_id")],
            "name",
        )];
        assert_eq!(
            s.join_stages(),
            vec![
                doc! { "$lookup": { "from": "clients", "localField": "client_id", "foreignField": "_id", "as": "__vj0" } },
                doc! { "$set": { "__vj0": { "$first": "$__vj0" } } },
                doc! { "$lookup": { "from": "countries", "localField": "__vj0.country_id", "foreignField": "_id", "as": "__vj1" } },
                doc! { "$set": { "__vj1": { "$first": "$__vj1" } } },
                doc! { "$addFields": { "__vc0": "$__vj1.name" } },
                doc! { "$unset": ["__vj0", "__vj1"] },
            ]
        );
    }

    #[test]
    fn columns_sharing_a_hop_share_its_lookup() {
        let mut s = MongoSelect::new();
        let client = hop("clients", "client_id");
        s.joins = vec![
            column("client.name", vec![client.clone()], "name"),
            column("client.city", vec![client], "address.city"),
        ];
        let stages = s.join_stages();
        assert_eq!(stages.len(), 4, "{stages:?}");
        assert_eq!(
            stages[2],
            doc! { "$addFields": { "__vc0": "$__vj0.name", "__vc1": "$__vj0.address.city" } }
        );
    }

    #[test]
    fn no_joins_no_stages() {
        assert!(MongoSelect::new().join_stages().is_empty());
    }

    #[test]
    fn alias_index_parses_only_aliases() {
        assert_eq!(join_alias(3), "__vc3");
        assert_eq!(alias_index("__vc3"), Some(3));
        assert_eq!(alias_index("__vc"), None);
        assert_eq!(alias_index("__vc1x"), None);
        assert_eq!(alias_index("name"), None);
    }

    #[test]
    fn filter_mentions_join_aliases_anywhere() {
        let filter = doc! { "$and": [ { "n": 1 }, { "__vc1": { "$regex": "a" } } ] };
        assert!(filter_uses_joins(&filter, 2));
        assert!(!filter_uses_joins(&filter, 1));
        assert!(!filter_uses_joins(&doc! { "__vc0": 1 }, 0));
        let expr = doc! { "$expr": { "$eq": ["$__vc0", "x"] } };
        assert!(filter_uses_joins(&expr, 1));
        assert!(!filter_uses_joins(&doc! { "client.name": "x" }, 1));
    }
}
