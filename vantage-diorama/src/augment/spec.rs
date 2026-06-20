//! Serde spec for declaring augmentations in config (YAML). Pure data — the
//! runtime form (closures, in [`super`]) is produced by [`super::lower_augment`].
//!
//! ```yaml
//! augment:
//!   - table: tfstate_detail          # catalog name → base detail vista (any persistence)
//!     source: { kind: column, from: key }
//!     fetch:  { kind: per_row }
//!     merge:  [resources, serial, outputs]
//! ```

use serde::Deserialize;

/// One declared augmentation: enrich each master row from another `table`.
#[derive(Debug, Clone, Deserialize)]
pub struct AugmentSpec {
    /// Catalog name of the detail model this augmentation reads from.
    pub table: String,
    /// How to narrow the detail vista for a given master row.
    pub source: SourceSpec,
    /// How to pull records from the narrowed detail vista.
    #[serde(default)]
    pub fetch: FetchSpec,
    /// Detail columns to lift onto the master row. Empty = lift all.
    #[serde(default)]
    pub merge: Vec<String>,
}

/// Key derivation: how a master row selects its detail record(s). Internally
/// tagged on `kind` so unit and struct variants coexist (`{kind: id}`,
/// `{kind: column, from: x}`, `{kind: script, code: "..."}`).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceSpec {
    /// `master.id → detail.id`.
    Id,
    /// `master[from] → detail[to | detail.id]`.
    Column {
        from: String,
        #[serde(default)]
        to: Option<String>,
    },
    /// Rhai narrows the base detail vista in place using `row` (requires the
    /// `rhai` feature). Per-row only — see [`super::Source::Build`].
    Script { code: String },
}

/// Fetch mode: how the narrowed detail vista is read.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FetchSpec {
    /// One detail record per master row.
    #[default]
    PerRow,
    /// Collect distinct keys across the window into one set query (phase 2).
    Batched {
        #[serde(default)]
        op: SetOp,
    },
    /// Rhai fetch verbs decide how to pull (phase 2).
    Script { code: String },
}

/// Set operator for [`FetchSpec::Batched`].
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetOp {
    /// `detail[col] IN [keys]`.
    #[default]
    Contains,
}
