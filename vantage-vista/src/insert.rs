//! Nested record insert.
//!
//! A flat insert sends one record to one table. This module lets a single
//! insert carry related records too: keys that name a **relation** (rather than
//! a column) hold the child data, and Vista sequences the writes so foreign
//! keys are populated automatically.
//!
//! - **has-one** (`inventory` / `inventory.count`): the child is inserted
//!   *first*, then its id is stamped into the parent's foreign-key column.
//! - **has-many** (`orders`): the parent is inserted *first*, then each child is
//!   inserted with the parent's id stamped into the child's foreign-key column.
//!
//! Vista does **no** field validation — the underlying table validates every
//! record it receives. Vista's only job here is to order the inserts and fill
//! the reference (FK) values. The sequence is **best-effort / non-atomic**: a
//! failure mid-way leaves earlier writes committed. Only **native**
//! (same-persistence) relations are supported; cross-persistence
//! ([`Vista::with_foreign`](crate::vista::Vista::with_foreign)) relations are
//! rejected.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::InsertableValueSet;
use vantage_types::Record;

use crate::{reference::ReferenceKind, vista::Vista};

/// A relation's child data, peeled off an insert record before classification.
enum Collected {
    /// A single child record — from a bare map value or grouped `rel.col` keys.
    Map(Record<CborValue>),
    /// An ordered list of child records — from a bare CBOR list value.
    List(Vec<Record<CborValue>>),
}

/// A has-one child to insert before the main row: `(relation, foreign_key, child)`.
type HasOneChild = (String, String, Record<CborValue>);
/// A has-many group to insert after the main row: `(relation, foreign_key, children)`.
type HasManyGroup = (String, String, Vec<Record<CborValue>>);

impl Vista {
    /// Split an insert record into the main row's fields and the per-relation
    /// child payloads, then classify each relation as has-one / has-many.
    ///
    /// Returns `(main, has_one, has_many)`. A relation key that resolves to no
    /// same-persistence reference (i.e. cross-persistence or unknown) is an
    /// error, raised before any write happens.
    fn classify_insert(
        &self,
        record: &Record<CborValue>,
    ) -> Result<(Record<CborValue>, Vec<HasOneChild>, Vec<HasManyGroup>)> {
        let relation_names: std::collections::HashSet<String> =
            self.list_references().into_iter().map(|(n, _)| n).collect();

        let mut main: Record<CborValue> = Record::new();
        let mut collected: IndexMap<String, Collected> = IndexMap::new();

        for (key, value) in record.iter() {
            // has-one shorthand: `relation.column = scalar`
            if let Some((prefix, rest)) = key.split_once('.')
                && relation_names.contains(prefix)
            {
                match collected
                    .entry(prefix.to_string())
                    .or_insert_with(|| Collected::Map(Record::new()))
                {
                    Collected::Map(child) => {
                        child.insert(rest.to_string(), value.clone());
                    }
                    Collected::List(_) => {
                        return Err(error!(
                            "relation given both a list and dotted fields",
                            relation = prefix
                        ));
                    }
                }
                continue;
            }

            // bare relation key: map (has-one) or list of maps (has-many)
            if relation_names.contains(key.as_str()) {
                match value {
                    CborValue::Map(_) => {
                        let incoming = Record::<CborValue>::from(value.clone());
                        match collected
                            .entry(key.clone())
                            .or_insert_with(|| Collected::Map(Record::new()))
                        {
                            Collected::Map(child) => {
                                for (k, v) in incoming {
                                    child.insert(k, v);
                                }
                            }
                            Collected::List(_) => {
                                return Err(error!(
                                    "relation given both a list and a map",
                                    relation = key
                                ));
                            }
                        }
                    }
                    CborValue::Array(items) => {
                        if collected.contains_key(key.as_str()) {
                            return Err(error!("relation given more than once", relation = key));
                        }
                        let mut children = Vec::with_capacity(items.len());
                        for item in items {
                            if !matches!(item, CborValue::Map(_)) {
                                return Err(error!(
                                    "has-many relation items must be maps",
                                    relation = key
                                ));
                            }
                            children.push(Record::<CborValue>::from(item.clone()));
                        }
                        collected.insert(key.clone(), Collected::List(children));
                    }
                    _ => {
                        return Err(error!(
                            "relation value must be a map (has-one) or a list of maps (has-many)",
                            relation = key
                        ));
                    }
                }
                continue;
            }

            // plain field → main row, untouched (the table validates it)
            main.insert(key.clone(), value.clone());
        }

        let mut has_one: Vec<HasOneChild> = Vec::new();
        let mut has_many: Vec<HasManyGroup> = Vec::new();
        for (relation, payload) in collected {
            let reference = self.get_reference(&relation).ok_or_else(|| {
                error!(
                    "cross-persistence nested insert is not supported",
                    relation = relation.as_str()
                )
            })?;
            let foreign_key = reference.foreign_key.clone();
            match reference.kind {
                ReferenceKind::HasOne => match payload {
                    Collected::Map(child) => has_one.push((relation, foreign_key, child)),
                    Collected::List(_) => {
                        return Err(error!(
                            "has-one relation expects a single record, got a list",
                            relation = relation.as_str()
                        ));
                    }
                },
                ReferenceKind::HasMany => match payload {
                    Collected::List(children) => has_many.push((relation, foreign_key, children)),
                    Collected::Map(_) => {
                        return Err(error!(
                            "has-many relation expects a list of records",
                            relation = relation.as_str()
                        ));
                    }
                },
            }
        }

        Ok((main, has_one, has_many))
    }

    /// Insert each has-one child into its bare target and stamp the returned id
    /// into the main record's foreign-key column. Run before the main row.
    async fn insert_has_one_children(
        &self,
        main: &mut Record<CborValue>,
        has_one: Vec<HasOneChild>,
    ) -> Result<()> {
        for (relation, foreign_key, child) in has_one {
            let target = self.get_ref_target(&relation)?;
            let child_id = target.insert_return_id_value(&child).await?;
            main.insert(foreign_key, CborValue::Text(child_id));
        }
        Ok(())
    }

    /// Insert each has-many child into its bare target with `parent_id` stamped
    /// into the child's foreign-key column. Run after the main row.
    async fn insert_has_many_children(
        &self,
        parent_id: &str,
        has_many: Vec<HasManyGroup>,
    ) -> Result<()> {
        for (relation, foreign_key, children) in has_many {
            let target = self.get_ref_target(&relation)?;
            for mut child in children {
                child.insert(foreign_key.clone(), CborValue::Text(parent_id.to_string()));
                target.insert_return_id_value(&child).await?;
            }
        }
        Ok(())
    }

    /// Nested insert returning the main row's id (the `insert_return_id_value`
    /// path). Children with auto-assigned ids; main row id chosen by the driver.
    pub(crate) async fn insert_nested_return_id(
        &self,
        record: &Record<CborValue>,
    ) -> Result<String> {
        let (mut main, has_one, has_many) = self.classify_insert(record)?;
        self.insert_has_one_children(&mut main, has_one).await?;
        let parent_id = self
            .source
            .insert_vista_return_id_value(self, &main)
            .await?;
        self.insert_has_many_children(&parent_id, has_many).await?;
        Ok(parent_id)
    }

    /// Nested insert for an explicitly-keyed main row (the `insert_value` path).
    /// Returns the inserted main record as the driver stored it.
    pub(crate) async fn insert_nested_value(
        &self,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let (mut main, has_one, has_many) = self.classify_insert(record)?;
        self.insert_has_one_children(&mut main, has_one).await?;
        let inserted = self.source.insert_vista_value(self, id, &main).await?;
        self.insert_has_many_children(id, has_many).await?;
        Ok(inserted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Column, Reference, ReferenceKind, VistaMetadata, mocks::mock_shell::MockShell};

    /// Vista over a `client` table: scalar columns plus a has-one `bakery`
    /// (FK `bakery_id` on this row) and a has-many `orders` (FK `client_id` on
    /// the order row).
    fn client_vista() -> Vista {
        let metadata = VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("name", "String"))
            .with_id_column("id")
            .with_reference(Reference::new(
                "bakery",
                "bakery",
                ReferenceKind::HasOne,
                "bakery_id",
            ))
            .with_reference(Reference::new(
                "orders",
                "order",
                ReferenceKind::HasMany,
                "client_id",
            ));
        Vista::new("client", Box::new(MockShell::new().with_metadata(metadata)))
    }

    fn text(s: &str) -> CborValue {
        CborValue::Text(s.into())
    }

    fn map(pairs: &[(&str, CborValue)]) -> CborValue {
        CborValue::Map(
            pairs
                .iter()
                .map(|(k, v)| (CborValue::Text((*k).into()), v.clone()))
                .collect(),
        )
    }

    fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), v.clone()))
            .collect()
    }

    #[test]
    fn flat_record_has_no_relations() {
        let (main, has_one, has_many) = client_vista()
            .classify_insert(&record(&[
                ("name", text("John")),
                ("bakery_id", text("b1")),
            ]))
            .unwrap();
        assert_eq!(main.get("name"), Some(&text("John")));
        assert_eq!(main.get("bakery_id"), Some(&text("b1")));
        assert!(has_one.is_empty());
        assert!(has_many.is_empty());
    }

    #[test]
    fn dotted_keys_group_into_one_has_one_child() {
        let (main, has_one, has_many) = client_vista()
            .classify_insert(&record(&[
                ("name", text("John")),
                ("bakery.name", text("New Bakery")),
                ("bakery.profit_margin", CborValue::Integer(10.into())),
            ]))
            .unwrap();
        assert_eq!(main.get("name"), Some(&text("John")));
        assert!(main.get("bakery.name").is_none());
        assert!(has_many.is_empty());
        assert_eq!(has_one.len(), 1);
        let (relation, fk, child) = &has_one[0];
        assert_eq!(relation, "bakery");
        assert_eq!(fk, "bakery_id");
        assert_eq!(child.get("name"), Some(&text("New Bakery")));
        assert_eq!(
            child.get("profit_margin"),
            Some(&CborValue::Integer(10.into()))
        );
    }

    #[test]
    fn bare_map_is_a_has_one_child() {
        let (_main, has_one, _has_many) = client_vista()
            .classify_insert(&record(&[("bakery", map(&[("name", text("New Bakery"))]))]))
            .unwrap();
        assert_eq!(has_one.len(), 1);
        assert_eq!(has_one[0].2.get("name"), Some(&text("New Bakery")));
    }

    #[test]
    fn bare_list_is_a_has_many_group() {
        let (_main, has_one, has_many) = client_vista()
            .classify_insert(&record(&[(
                "orders",
                CborValue::Array(vec![
                    map(&[("total", CborValue::Integer(1.into()))]),
                    map(&[("total", CborValue::Integer(2.into()))]),
                ]),
            )]))
            .unwrap();
        assert!(has_one.is_empty());
        assert_eq!(has_many.len(), 1);
        let (relation, fk, children) = &has_many[0];
        assert_eq!(relation, "orders");
        assert_eq!(fk, "client_id");
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn has_one_given_a_list_is_an_error() {
        let err = client_vista()
            .classify_insert(&record(&[("bakery", CborValue::Array(vec![map(&[])]))]));
        assert!(err.is_err());
    }

    #[test]
    fn has_many_given_a_map_is_an_error() {
        let err =
            client_vista().classify_insert(&record(&[("orders", map(&[("total", text("x"))]))]));
        assert!(err.is_err());
    }

    #[test]
    fn cross_persistence_relation_is_rejected() {
        let mut vista = client_vista();
        vista.with_foreign("warehouse", ReferenceKind::HasOne, |_row| {
            Err(vantage_core::error!("unused in this test"))
        });
        // `warehouse` resolves via list_references but has no same-persistence
        // Reference, so classify must reject it before any write.
        let err = vista.classify_insert(&record(&[("warehouse", map(&[("name", text("w1"))]))]));
        assert!(err.is_err());
    }
}
