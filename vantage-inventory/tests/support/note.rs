//! Toy kinds for the registry tests: `note/<key>.yaml` files whose
//! `refers_to:` must name another note, and a `settings.yaml` root file.

#![allow(dead_code)]

use std::path::Path;

use schemars::JsonSchema;
use serde::Deserialize;
use vantage_inventory::{
    Depth, Document, IndexSink, KeySnapshot, Kind, Location, Missing, OnFailure, Stage,
    validate_into,
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub title: String,
    #[serde(default)]
    pub refers_to: Option<String>,
}

pub struct NoteKind {
    pub id: &'static str,
    pub depth: Depth,
    pub on_failure: fn(Stage, &anyhow::Error) -> OnFailure,
    pub broken_schema: bool,
}

impl NoteKind {
    pub fn flat() -> Self {
        Self {
            id: "note",
            depth: Depth::Flat,
            on_failure: |_, _| OnFailure::Keep,
            broken_schema: false,
        }
    }

    pub fn one_level() -> Self {
        Self {
            depth: Depth::OneLevel,
            ..Self::flat()
        }
    }
}

pub const NOTE_README: &str = "# note/\n\nOne note per file.\n";

impl Kind for NoteKind {
    type Value = Note;

    fn id(&self) -> &'static str {
        self.id
    }

    fn location(&self) -> Location {
        Location::Folder {
            name: "note",
            depth: self.depth,
        }
    }

    fn parse(&self, _key: &str, doc: &Document) -> anyhow::Result<Note> {
        validate_into(doc)
    }

    fn schema(&self) -> anyhow::Result<serde_json::Value> {
        if self.broken_schema {
            anyhow::bail!("note schema unavailable");
        }
        Ok(vantage_inventory::schema_for_type::<Note>()?.json.clone())
    }

    fn validate(&self, key: &str, v: &Note, keys: &KeySnapshot) -> Vec<String> {
        match &v.refers_to {
            Some(target) if !keys.has("note", target) => {
                vec![format!("{key}: refers_to `{target}` is not a note")]
            }
            _ => vec![],
        }
    }

    fn index(&self, key: &str, v: &Note, out: &mut IndexSink) {
        if let Some(target) = &v.refers_to {
            out.push("referrers", target, key);
        }
    }

    fn readme(&self) -> Option<&'static str> {
        Some(NOTE_README)
    }

    fn on_reload_failure(&self, stage: Stage, error: &anyhow::Error) -> OnFailure {
        (self.on_failure)(stage, error)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub theme: String,
}

pub fn settings(missing: Missing) -> SettingsKind {
    SettingsKind {
        id: "settings",
        file: "settings.yaml",
        missing,
    }
}

/// `settings.yaml` at the project root; `missing` picks what an absent
/// file means.
pub struct SettingsKind {
    pub id: &'static str,
    pub file: &'static str,
    pub missing: Missing,
}

impl Kind for SettingsKind {
    type Value = Settings;

    fn id(&self) -> &'static str {
        self.id
    }

    fn location(&self) -> Location {
        Location::RootFile(self.file)
    }

    fn parse(&self, _key: &str, doc: &Document) -> anyhow::Result<Settings> {
        validate_into(doc)
    }

    fn schema(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({ "type": "object" }))
    }

    fn on_missing(&self) -> Missing {
        self.missing
    }
}

pub fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}
