//! [`VistaCapabilities`]: the set of capabilities a shell advertises.

use std::fmt;

use serde::de::{Deserializer, MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

use super::{ALL, Capability};

/// The set of capabilities a shell advertises.
///
/// Serializes as a map from every capability's name to whether the set has
/// it, so a reader (rhai, MCP) sees the full vocabulary. Deserializing
/// rejects a name it doesn't know, so a typo or a retired name fails loudly
/// instead of leaving the capability off.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct VistaCapabilities {
    bits: u16,
}

impl VistaCapabilities {
    /// The empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// This set plus `capability`.
    pub fn with(mut self, capability: Capability) -> Self {
        self.bits |= capability.bit();
        self
    }

    /// This set without `capability`.
    pub fn without(mut self, capability: Capability) -> Self {
        self.bits &= !capability.bit();
        self
    }

    pub fn has(&self, capability: Capability) -> bool {
        self.bits & capability.bit() != 0
    }

    /// The capabilities in the set, in [`Capability::iter`] order.
    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        Capability::iter().filter(|c| self.has(*c))
    }
}

impl FromIterator<Capability> for VistaCapabilities {
    fn from_iter<I: IntoIterator<Item = Capability>>(iter: I) -> Self {
        iter.into_iter().fold(Self::new(), Self::with)
    }
}

impl fmt::Debug for VistaCapabilities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl Serialize for VistaCapabilities {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(ALL.len()))?;
        for capability in Capability::iter() {
            map.serialize_entry(capability.as_str(), &self.has(capability))?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for VistaCapabilities {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SetVisitor;
        impl<'de> Visitor<'de> for SetVisitor {
            type Value = VistaCapabilities;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map from capability name to bool")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut set = VistaCapabilities::new();
                while let Some((capability, on)) = map.next_entry::<Capability, bool>()? {
                    if on {
                        set = set.with(capability);
                    }
                }
                Ok(set)
            }
        }
        deserializer.deserialize_map(SetVisitor)
    }
}
