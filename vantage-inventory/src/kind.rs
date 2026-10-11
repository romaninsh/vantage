//! What one kind of inventory file is: where its files live, how a loaded
//! [`Document`] becomes a typed value, and how that value is checked
//! against the rest of the project.
//!
//! An [`Inventory`](crate::Inventory) holds any number of kinds. It walks
//! each kind's [`Location`], parses every file it finds with
//! [`Kind::parse`], and then runs [`Kind::validate`] for every leaf against
//! a [`KeySnapshot`] of the whole project. Validation produces messages,
//! not errors: an invalid leaf still loads, so an app can show it alongside
//! what is wrong with it.

use crate::Document;
use crate::keys::{IndexSink, KeySnapshot};

/// How deep a folder kind looks for files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// `<folder>/<key>.yaml` only; subfolders are ignored.
    Flat,
    /// Also `<folder>/<sub>/<stem>.yaml`, keyed `<sub>/<stem>`. Nothing
    /// deeper is read.
    OneLevel,
}

/// Where a kind's files live, relative to the project root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    /// One leaf per `.yaml` file in the folder, keyed by the file stem.
    Folder { name: &'static str, depth: Depth },
    /// A single file at the root, such as `application.yaml`. Its one leaf
    /// is keyed by the file stem.
    RootFile(&'static str),
}

/// What an absent [`Location::RootFile`] means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// No file, no leaf.
    Absent,
    /// The leaf always exists. When the file is absent or does not parse,
    /// its value is `None` and the caller substitutes its default.
    Default,
}

/// The step a file failed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Reading, YAML parsing or `!include` resolution: there is no
    /// [`Document`].
    Load,
    /// [`Kind::parse`] rejected the document.
    Parse,
}

/// What a reload does with an existing leaf whose file stopped loading or
/// parsing. See [`Kind::on_reload_failure`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnFailure {
    /// Leave the leaf exactly as it was: value, serial, hash, includes and
    /// errors. Saving the same broken text again retries the parse.
    Keep,
    /// Keep the value and serial, but replace the leaf's errors with these
    /// messages and remember the broken file's hash, so restoring the last
    /// good text reloads and clears them. Revalidation leaves the messages
    /// alone until a reload succeeds.
    Report(Vec<String>),
    /// Drop the leaf, as if the file were deleted.
    Remove,
}

/// One kind of inventory file. Implement this for a unit struct per kind
/// and register it with [`Inventory::with`](crate::Inventory::with).
pub trait Kind: Send + Sync + 'static {
    /// What a successfully parsed file becomes.
    type Value: Send + Sync + 'static;

    /// Stable identifier, used as the `kind` in [`KeySnapshot`] and
    /// [`Change`](crate::Change). Unique within an inventory.
    fn id(&self) -> &'static str;
    fn location(&self) -> Location;
    /// Turn the loaded document of `key` into a value. Schema validation
    /// belongs here (see [`crate::validate_into`]); cross-references belong
    /// in [`Kind::validate`].
    fn parse(&self, key: &str, doc: &Document) -> anyhow::Result<Self::Value>;
    /// The JSON schema written beside the files by
    /// [`Inventory::emit_schemas`](crate::Inventory::emit_schemas). An
    /// error stops emission.
    fn schema(&self) -> anyhow::Result<serde_json::Value>;
    /// Check `v` against every key in the project. Each message is one
    /// problem; none means valid.
    fn validate(&self, _key: &str, _v: &Self::Value, _keys: &KeySnapshot) -> Vec<String> {
        vec![]
    }
    /// Contribute named lookup tables that other kinds' validators read
    /// through [`KeySnapshot::index`].
    fn index(&self, _key: &str, _v: &Self::Value, _out: &mut IndexSink) {}
    /// Text for the `README.md` written into the kind's folder.
    fn readme(&self) -> Option<&'static str> {
        None
    }
    fn on_missing(&self) -> Missing {
        Missing::Absent
    }
    /// Called when a reload of an existing leaf fails at `stage`. A file
    /// that fails on its first load never becomes a leaf, whatever this
    /// returns; the failure is only returned (see
    /// [`Inventory::load`](crate::Inventory::load)).
    fn on_reload_failure(&self, _stage: Stage, _error: &anyhow::Error) -> OnFailure {
        OnFailure::Keep
    }
}
