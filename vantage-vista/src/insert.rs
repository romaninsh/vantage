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
//! failure mid-way leaves earlier writes committed. Every Vista reference is
//! same-persistence, so every single-key join is insertable. Contained
//! relations are not relations here: their data is the host column's value.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::InsertableValueSet;
use vantage_types::Record;

use crate::{
    reference::{Reference, ReferenceKind},
    vista::Vista,
};

#[cfg(test)]
mod tests;

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

/// The single foreign-key column a nested insert stamps.
fn foreign_key(reference: &Reference) -> Result<String> {
    reference.foreign_key().map(str::to_string).ok_or_else(|| {
        error!(
            "nested insert needs a relation joined on one key",
            relation = reference.name.as_str()
        )
    })
}

impl Vista {
    /// Split an insert record into the main row's fields and the per-relation
    /// child payloads, then classify each relation as has-one / has-many.
    ///
    /// Returns `(main, has_one, has_many)`. A relation key that resolves to no
    /// join is an error, raised before any write happens.
    fn classify_insert(
        &self,
        record: &Record<CborValue>,
    ) -> Result<(Record<CborValue>, Vec<HasOneChild>, Vec<HasManyGroup>)> {
        let relation_names: std::collections::HashSet<&str> = self
            .references()
            .values()
            .filter(|r| !r.kind.is_contained())
            .map(|r| r.name.as_str())
            .collect();

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
                    // A scalar names an EXISTING record: link it by
                    // stamping the foreign-key column — the same column a
                    // co-inserted child's id would land in. The value's
                    // shape (Text id, Tag record-id, integer key, null)
                    // is the driver's to validate, like any plain field.
                    _ => {
                        let reference = self.join_reference(key)?;
                        if reference.kind != ReferenceKind::HasOne {
                            return Err(error!(
                                "has-many relation takes a list of maps, not a scalar",
                                relation = key
                            ));
                        }
                        self.check_insert_link(key)?;
                        main.insert(foreign_key(reference)?, value.clone());
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
            let reference = self.join_reference(&relation)?;
            let foreign_key = foreign_key(reference)?;
            self.check_insert_link(&relation)?;
            match reference.kind {
                // Inserting a child stamps its new id into the main row's
                // foreign key, so a main row that ALREADY carries that
                // column is two instructions in one record: link this
                // existing row, and create a new one to link instead.
                // Silently, the child would win. Refuse instead — whether
                // the column arrived as a scalar under the relation name
                // (`bakery: "bakery:x"`) or as the plain column
                // (`bakery_id: "x"`), neither is a typo worth guessing at.
                ReferenceKind::HasOne if main.get(&foreign_key).is_some() => {
                    return Err(error!(
                        "relation given both a record link and child data",
                        relation = relation.as_str(),
                        foreign_key = foreign_key.as_str()
                    ));
                }
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
                _ => {
                    return Err(error!(
                        "relation has no insertable reference",
                        relation = relation.as_str()
                    ));
                }
            }
        }

        Ok((main, has_one, has_many))
    }

    fn join_reference(&self, relation: &str) -> Result<&Reference> {
        self.reference(relation)
            .ok_or_else(|| error!("relation has no insertable reference", relation = relation))
    }

    /// Insert each has-one child into its bare target and stamp the returned id
    /// into the main record's foreign-key column. Run before the main row.
    async fn insert_has_one_children(
        &self,
        main: &mut Record<CborValue>,
        has_one: Vec<HasOneChild>,
    ) -> Result<()> {
        for (relation, foreign_key, child) in has_one {
            let target = self.get_ref(&relation, None)?;
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
            let target = self.get_ref(&relation, None)?;
            for mut child in children {
                child.insert(foreign_key.clone(), CborValue::Text(parent_id.to_string()));
                target.insert_return_id_value(&child).await?;
            }
        }
        Ok(())
    }

    /// Nested insert under a backend-made id (the `insert_return_id_value`
    /// path). Returns that id.
    pub(crate) async fn insert_nested_return_id(
        &self,
        record: &Record<CborValue>,
    ) -> Result<String> {
        let (mut main, has_one, has_many) = self.classify_insert(&self.without_computed(record))?;
        self.insert_has_one_children(&mut main, has_one).await?;
        let (parent_id, _) = self.shell().insert(None, &main).await?;
        self.insert_has_many_children(&parent_id, has_many).await?;
        Ok(parent_id)
    }

    /// Nested insert for an explicitly-keyed main row (the `insert_value` path).
    /// Returns the inserted main record as the driver stored it.
    pub(crate) async fn insert_nested_value(
        &self,
        id: &str,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let (mut main, has_one, has_many) = self.classify_insert(&self.without_computed(record))?;
        self.insert_has_one_children(&mut main, has_one).await?;
        let (_, inserted) = self.shell().insert(Some(id), &main).await?;
        self.insert_has_many_children(id, has_many).await?;
        Ok(inserted)
    }
}
