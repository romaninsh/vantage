//! The contract a compiled-in model crate implements to serve its
//! tables and actions to an application.
//!
//! A Vantage application reads its screens from YAML, and its tables
//! from whatever the YAML declares. A *bundle* is the other source: a
//! Rust crate, compiled into the binary, that hands the application
//! typed tables and the operations that go with them. The application
//! resolves a bundle table exactly like a declared one — pages, menus,
//! relations and scripting cannot tell them apart.
//!
//! # Why this is its own crate
//!
//! The traits are small, but which side of the dependency they sit on
//! decides how a client model ships. Keeping them inside the
//! application means the model crate must depend on the application to
//! implement them, which it cannot: the application is a binary, and a
//! model belongs to whoever owns the data. So the application's build
//! grows a branch per client — a Cargo feature, an optional path
//! dependency, and a hand-written shim — and every release has to be
//! merged onto each one.
//!
//! Published here instead, the arrow turns around. A model crate
//! depends on this, implements [`ModelBundle`], and any application can
//! compile it in with no knowledge of that model beyond its name.
//!
//! # What a bundle provides
//!
//! - [`ModelBundle::tables`] — the tables, by catalog key and the
//!   datasource whose live connection they use. Note there is no schema
//!   here: the application builds each table's [`Vista`] against an
//!   offline connection and reads the columns, flags and references off
//!   it, so the model crate stays the single source of truth and cannot
//!   drift from a copy.
//! - [`ModelBundle::model_actions`] — typed operations carrying their
//!   own input and output schemas ([`vantage_action::ModelAction`]).
//!   The application synthesizes a form from the input schema, so the
//!   screen declaring the action does not restate its fields.
//! - [`ModelBundle::actions`] — the older, untyped [`BuiltinAction`]:
//!   JSON in, JSON out, with the fields declared by the screen. Kept
//!   for bodies that predate model actions; prefer `model_actions` for
//!   anything new.

use std::collections::HashMap;
use std::sync::Arc;

use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_vista::Vista;

/// One table a bundle provides.
///
/// There is deliberately no schema: see the note on
/// [`ModelBundle::tables`].
pub struct BundleTable {
    /// Catalog key the table is reachable under (`tag`, `batch`, …).
    pub key: String,
    /// Datasource key (e.g. `golf`) whose live connection this table
    /// uses. Named, not held — the application owns connections and
    /// hands the right one to [`ModelBundle::build_vista`].
    pub datasource: String,
    /// Display title for the synthesized config.
    pub title: Option<String>,
}

/// A compiled-in model crate that provides tables and operations to an
/// application.
pub trait ModelBundle: Send + Sync {
    /// Bundle name, for logs.
    fn name(&self) -> &str;

    /// The tables this bundle provides.
    ///
    /// Only keys and datasources — the application introspects each
    /// table's real shape from the [`Vista`] that
    /// [`ModelBundle::build_vista`] returns, rather than from anything
    /// restated here. A description that could disagree with the model
    /// eventually does.
    fn tables(&self) -> Vec<BundleTable>;

    /// Build the typed Vista for one table, given the live connection
    /// of the datasource its [`BundleTable`] named.
    fn build_vista(&self, table_key: &str, db: &SurrealDB) -> vantage_core::Result<Vista>;

    /// Typed operations this bundle provides, built against the live
    /// connections of an open project.
    ///
    /// Called on every (re)connect, so a bundle sees the connections as
    /// they are now. Return an empty vec when a datasource an operation
    /// needs is absent — the application reports the operation as
    /// unavailable rather than offering one that cannot run.
    fn model_actions(
        &self,
        surreal: &HashMap<String, SurrealDB>,
    ) -> Vec<Arc<dyn vantage_action::ModelAction>> {
        let _ = surreal;
        Vec::new()
    }

    /// Untyped builtin actions. Prefer [`ModelBundle::model_actions`].
    fn actions(&self) -> Vec<Arc<dyn BuiltinAction>> {
        Vec::new()
    }
}

/// A compiled-in action body with no schema of its own.
///
/// The screen declaring it owns the contract — dialog, form fields,
/// placement — and this is only what runs when the user confirms.
/// [`vantage_action::ModelAction`] supersedes it: an action that
/// describes its own input can be rendered, called over HTTP and
/// scripted from one definition, where this one can only be rendered
/// from a screen that already knows its fields.
#[async_trait::async_trait]
pub trait BuiltinAction: Send + Sync {
    /// The name a screen references.
    fn name(&self) -> &str;

    /// Datasource key whose live connection this action writes through.
    fn datasource(&self) -> &str;

    /// Run the action. `args` is a JSON object of the confirmed form
    /// values keyed by field name; the JSON result becomes the record
    /// the calling script receives.
    async fn run(
        &self,
        db: &SurrealDB,
        args: serde_json::Value,
    ) -> vantage_core::Result<serde_json::Value>;
}
