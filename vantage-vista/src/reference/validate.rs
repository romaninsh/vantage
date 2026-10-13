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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> Result<Reference, String> {
        serde_yaml_ng::from_str::<Reference>(yaml).map_err(|e| e.to_string())
    }

    #[test]
    fn a_valid_join_and_contained_relation_round_trip() {
        let join = Reference::has_many("orders", "order", JoinKey::new("id", "client_id"))
            .with_key(JoinKey::new("region", "region"))
            .with_build_script("table(\"order\")")
            .unwrap();
        let contained = Reference::contains_many("lines", "lines");
        for reference in [join, contained] {
            let yaml = serde_yaml_ng::to_string(&reference).unwrap();
            assert_eq!(parse(&yaml).unwrap(), reference, "{yaml}");
        }
    }

    #[test]
    fn a_join_needs_a_key_and_no_host_column() {
        let keyless = parse("name: client\nkind: has_one\ntarget: client\n").unwrap_err();
        assert!(keyless.contains("needs at least one key"), "{keyless}");
        assert!(keyless.contains("client"), "{keyless}");

        let hosted = parse(
            "name: client\nkind: has_one\ntarget: client\nkeys: [{from: client_id}]\n\
             host_column: c\n",
        )
        .unwrap_err();
        assert!(hosted.contains("takes no host_column"), "{hosted}");
    }

    #[test]
    fn a_contained_relation_needs_a_host_column_and_no_keys_or_script() {
        let base = "name: lines\nkind: contains_many\ntarget: lines\n";
        let unhosted = parse(base).unwrap_err();
        assert!(unhosted.contains("needs a host_column"), "{unhosted}");

        let keyed = parse(&format!("{base}host_column: lines\nkeys: [{{from: id}}]\n"));
        assert!(keyed.unwrap_err().contains("takes no join keys"));

        let scripted = parse(&format!("{base}host_column: lines\nbuild_script: x\n"));
        let scripted = scripted.unwrap_err();
        assert!(scripted.contains("allowed on joins only"), "{scripted}");
        assert!(scripted.contains("lines"), "{scripted}");
    }

    /// The builder refuses a build script on a contained kind with the same
    /// error deserialising gives.
    #[test]
    fn a_build_script_on_a_contained_relation_is_an_error() {
        let err = Reference::contains_one("lines", "lines")
            .with_build_script("x")
            .unwrap_err();
        assert!(
            !err.is_unsupported() && !err.is_incorrect_usage(),
            "{err:?}"
        );
        assert!(
            err.to_string()
                .contains("a build_script is allowed on joins only"),
            "{err}"
        );
        assert_eq!(
            err.context.get("relation").map(String::as_str),
            Some("\"lines\"")
        );
    }

    #[test]
    fn kinds_render_as_yaml_spells_them() {
        for kind in [
            ReferenceKind::HasOne,
            ReferenceKind::HasMany,
            ReferenceKind::ContainsOne,
            ReferenceKind::ContainsMany,
        ] {
            let yaml = serde_yaml_ng::to_string(&kind).unwrap();
            assert_eq!(yaml.trim(), kind.as_str());
        }
    }
}
