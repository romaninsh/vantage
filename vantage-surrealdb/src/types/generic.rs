use crate::types::{SurrealType, SurrealTypeArrayMarker, SurrealTypeObjectMarker};
use ciborium::Value as CborValue;
use indexmap::IndexMap;

impl<T: SurrealType> SurrealType for Vec<T> {
    type Target = SurrealTypeArrayMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Array(self.iter().map(T::to_cbor).collect())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Array(a) => a.into_iter().map(T::from_cbor).collect(),
            _ => None,
        }
    }
}

impl<T: SurrealType> SurrealType for IndexMap<String, T> {
    type Target = SurrealTypeObjectMarker;

    fn to_cbor(&self) -> CborValue {
        let map: Vec<(CborValue, CborValue)> = self
            .iter()
            .map(|(k, v)| (CborValue::Text(k.clone()), v.to_cbor()))
            .collect();
        CborValue::Map(map)
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Map(m) => {
                let mut index_map = IndexMap::new();
                for (k, v) in m {
                    if let CborValue::Text(key) = k {
                        if let Some(value) = T::from_cbor(v) {
                            index_map.insert(key, value);
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                Some(index_map)
            }
            _ => None,
        }
    }
}

impl<T: SurrealType> SurrealType for Option<T> {
    type Target = T::Target;

    fn to_cbor(&self) -> ciborium::Value {
        match self {
            Some(s) => s.to_cbor(),
            None => ciborium::Value::Tag(6, Box::new(ciborium::Value::Null)),
        }
    }

    fn from_cbor(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Tag(6, _) => Some(None),
            s => T::from_cbor(s).map(Some),
        }
    }
}

impl SurrealType for crate::AnySurrealType {
    type Target = SurrealTypeObjectMarker; // Use object marker as default

    fn to_cbor(&self) -> CborValue {
        self.value().clone()
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        Self::from_cbor(&cbor)
    }
}
