//! The contract a compiled-in model crate implements to serve its
//! tables and actions to an application.
//!
//! A *bundle* is a Rust crate, compiled into the binary, that hands the
//! application typed tables and the operations that go with them. The
//! application resolves a bundle table exactly like one declared in YAML.
//!
//! The traits live in their own crate, above `vantage-vista` and
//! `vantage-action`, so a model crate can implement them without
//! depending on an application.
//!
//! [`ModelBundle::Connection`] is an associated type: a bundle names its
//! own database handle, and the application holds the matching
//! connections (`dyn ModelBundle<Connection = SurrealDB>`).
//!
//! A bundle provides [`ModelBundle::tables`] (keys and datasources, no
//! schema: the application reads columns off each table's [`Vista`]),
//! [`ModelBundle::model_actions`] (typed, self-describing operations) and
//! the older untyped [`ModelBundle::actions`].

use std::collections::HashMap;
use std::sync::Arc;

use vantage_vista::Vista;

/// One table a bundle provides. Carries no schema: see
/// [`ModelBundle::tables`].
pub struct BundleTable {
    /// Catalog key the table is reachable under (`tag`, `batch`, …).
    pub key: String,
    /// Datasource key (e.g. `golf`) whose connection this table uses. The
    /// application owns connections and passes the right one to
    /// [`ModelBundle::build_vista`].
    pub datasource: String,
    /// Display title for the synthesized config.
    pub title: Option<String>,
}

/// A compiled-in model crate that provides tables and operations to an
/// application.
pub trait ModelBundle: Send + Sync {
    /// The database handle this bundle's tables are built against. The
    /// application must hold connections of this type to serve the
    /// bundle — see the module docs.
    type Connection;

    /// Bundle name, for logs.
    fn name(&self) -> &str;

    /// The tables this bundle provides.
    ///
    /// Only keys and datasources: the application introspects each
    /// table's shape from the [`Vista`] that [`ModelBundle::build_vista`]
    /// returns, so the model stays the single source of truth.
    fn tables(&self) -> Vec<BundleTable>;

    /// Build the typed Vista for one table, given the live connection
    /// of the datasource its [`BundleTable`] named.
    fn build_vista(&self, table_key: &str, db: &Self::Connection) -> vantage_core::Result<Vista>;

    /// Typed operations this bundle provides, built against the live
    /// connections of an open project, keyed by datasource.
    ///
    /// Called on every (re)connect, so a bundle sees the connections as
    /// they are now. Return an empty vec when a datasource an operation
    /// needs is absent — the application reports the operation as
    /// unavailable rather than offering one that cannot run.
    fn model_actions(
        &self,
        connections: &HashMap<String, Self::Connection>,
    ) -> Vec<Arc<dyn vantage_action::ModelAction>> {
        let _ = connections;
        Vec::new()
    }

    /// Untyped builtin actions. Prefer [`ModelBundle::model_actions`].
    fn actions(&self) -> Vec<Arc<dyn BuiltinAction<Connection = Self::Connection>>> {
        Vec::new()
    }
}

/// A compiled-in action body with no schema of its own: the screen that
/// declares it owns the form fields. Superseded by
/// [`vantage_action::ModelAction`], which describes its own input.
#[async_trait::async_trait]
pub trait BuiltinAction: Send + Sync {
    /// The database handle this action writes through. Matches the
    /// bundle's [`ModelBundle::Connection`].
    type Connection;

    /// The name a screen references.
    fn name(&self) -> &str;

    /// Datasource key whose connection this action writes through.
    fn datasource(&self) -> &str;

    /// Run the action. `args` is a JSON object of the confirmed form
    /// values keyed by field name; the JSON result becomes the record
    /// the calling script receives.
    async fn run(
        &self,
        db: &Self::Connection,
        args: serde_json::Value,
    ) -> vantage_core::Result<serde_json::Value>;
}
