//! [`DynKind`]: the object-safe face of [`Kind`], so kinds of different
//! value types can sit in one `Vec<Box<dyn DynKind>>`. Every `Kind` is a
//! `DynKind` through the blanket impl; nothing implements it by hand.

use std::any::{Any, TypeId};
use std::sync::Arc;

use crate::Document;
use crate::keys::{IndexSink, KeySnapshot};
use crate::kind::{Kind, Location, Missing, OnFailure, Stage};

/// A parsed value with its type erased.
pub type AnyValue = Arc<dyn Any + Send + Sync>;

/// [`Kind`] with the value type erased. The `*_any` methods take and
/// return values produced by this same kind's [`DynKind::parse_any`]; a
/// value of any other type is ignored (validation returns nothing).
pub trait DynKind: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    fn location(&self) -> Location;
    fn schema(&self) -> anyhow::Result<serde_json::Value>;
    fn readme(&self) -> Option<&'static str>;
    fn on_missing(&self) -> Missing;
    fn on_reload_failure(&self, stage: Stage, error: &anyhow::Error) -> OnFailure;
    fn parse_any(&self, key: &str, doc: &Document) -> anyhow::Result<AnyValue>;
    fn validate_any(&self, key: &str, value: &AnyValue, keys: &KeySnapshot) -> Vec<String>;
    fn index_any(&self, key: &str, value: &AnyValue, out: &mut IndexSink);
    /// `TypeId` of the implementing [`Kind`], which is how
    /// [`Inventory::get`](crate::Inventory::get) finds it.
    fn kind_type(&self) -> TypeId;
}

impl<K: Kind> DynKind for K {
    fn id(&self) -> &'static str {
        Kind::id(self)
    }

    fn location(&self) -> Location {
        Kind::location(self)
    }

    fn schema(&self) -> anyhow::Result<serde_json::Value> {
        Kind::schema(self)
    }

    fn readme(&self) -> Option<&'static str> {
        Kind::readme(self)
    }

    fn on_missing(&self) -> Missing {
        Kind::on_missing(self)
    }

    fn on_reload_failure(&self, stage: Stage, error: &anyhow::Error) -> OnFailure {
        Kind::on_reload_failure(self, stage, error)
    }

    fn parse_any(&self, key: &str, doc: &Document) -> anyhow::Result<AnyValue> {
        Ok(Arc::new(self.parse(key, doc)?))
    }

    fn validate_any(&self, key: &str, value: &AnyValue, keys: &KeySnapshot) -> Vec<String> {
        match value.downcast_ref::<K::Value>() {
            Some(v) => self.validate(key, v, keys),
            None => vec![],
        }
    }

    fn index_any(&self, key: &str, value: &AnyValue, out: &mut IndexSink) {
        if let Some(v) = value.downcast_ref::<K::Value>() {
            self.index(key, v, out);
        }
    }

    fn kind_type(&self) -> TypeId {
        TypeId::of::<K>()
    }
}
