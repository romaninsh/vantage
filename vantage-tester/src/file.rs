mod clock;
pub(crate) mod validate;
mod values;

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;

pub use clock::{ClockOffset, ClockUnit};
pub use values::{Do, RecordSource, Session, VarDef, WhatIf};

/// Stub answers by action name: the value a call returns, or `{ fails: "…" }`.
pub type Stubs = IndexMap<String, Value>;

/// One scenario file, `tests/scenarios/<name>.yaml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioFile {
    /// The project-relative, normalised path set by [`crate::load`] or [`crate::parse`]. The
    /// per-scenario seed hashes it.
    #[serde(skip)]
    pub path: String,
    #[serde(default)]
    pub fixtures: OneOrMany<String>,
    #[serde(default)]
    pub vars: IndexMap<String, VarDef>,
    pub scenarios: IndexMap<String, Scenario>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub session: Option<Session>,
    /// Absolute start time, carried as text for the host to parse.
    pub clock: Option<String>,
    pub extends: Option<String>,
    #[serde(default)]
    pub stub: Stubs,
    pub skip: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub step: String,
    pub clock: Option<ClockOffset>,
    #[serde(rename = "as")]
    pub as_: Option<Session>,
    pub before: Option<String>,
    #[serde(rename = "do")]
    pub do_: Option<Do>,
    pub assert: Option<OneOrMany<String>>,
    pub fails: Option<String>,
    #[serde(default)]
    pub vars: IndexMap<String, VarDef>,
    #[serde(default)]
    pub stub: Stubs,
    pub what_if: Option<WhatIf>,
}

/// A YAML value written either as one item or as a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneOrMany<T>(pub Vec<T>);

impl<T> Default for OneOrMany<T> {
    fn default() -> Self {
        OneOrMany(Vec::new())
    }
}

impl<T> std::ops::Deref for OneOrMany<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for OneOrMany<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw<T> {
            Many(Vec<T>),
            One(T),
        }
        Ok(match Raw::deserialize(d)? {
            Raw::Many(v) => OneOrMany(v),
            Raw::One(v) => OneOrMany(vec![v]),
        })
    }
}
