//! Deserialising a [`Reference`]: the kind and its fields must agree.

use serde::Deserialize;
use vantage_core::{VantageError, error};

use super::{JoinKey, Reference, ReferenceKind};

/// A [`Reference`] as written, before its fields are checked against its
/// kind.
#[derive(Deserialize)]
pub(super) struct RawReference {
    name: String,
    kind: ReferenceKind,
    target: String,
    #[serde(default)]
    keys: Vec<JoinKey>,
    #[serde(default)]
    host_column: Option<String>,
    #[serde(default)]
    build_script: Option<String>,
}

impl TryFrom<RawReference> for Reference {
    type Error = VantageError;

    fn try_from(raw: RawReference) -> Result<Self, Self::Error> {
        let problem = if raw.kind.is_contained() {
            if raw.host_column.is_none() {
                Some("a contained relation needs a host_column")
            } else if !raw.keys.is_empty() {
                Some("a contained relation takes no join keys")
            } else {
                None
            }
        } else if raw.keys.is_empty() {
            Some("a join relation needs at least one key")
        } else if raw.host_column.is_some() {
            Some("a join relation takes no host_column")
        } else {
            None
        };
        if let Some(problem) = problem {
            return Err(mismatch(problem, &raw.name, raw.kind));
        }
        if raw.build_script.is_some() {
            check_build_script(&raw.name, raw.kind)?;
        }
        Ok(Reference {
            name: raw.name,
            kind: raw.kind,
            target: raw.target,
            keys: raw.keys,
            host_column: raw.host_column,
            build_script: raw.build_script,
        })
    }
}

/// Whether relation `name` of `kind` may carry a build script: only a join
/// has a traversal target to build.
pub(super) fn check_build_script(name: &str, kind: ReferenceKind) -> Result<(), VantageError> {
    if kind.is_contained() {
        return Err(mismatch(
            "a build_script is allowed on joins only",
            name,
            kind,
        ));
    }
    Ok(())
}

/// A plain error stating why `kind` and its fields disagree on `relation`.
fn mismatch(problem: &'static str, relation: &str, kind: ReferenceKind) -> VantageError {
    error!(problem, relation = relation, kind = kind.as_str())
}
