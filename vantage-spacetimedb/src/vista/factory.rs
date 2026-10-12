//! Building a `Vista` from a module's own schema.

use vantage_core::{Result, error};
use vantage_vista::{NoExtras, Vista, VistaFactory, VistaMetadata};

use crate::client::SpacetimeDb;
use crate::schema::ModuleSchema;
use crate::vista::capabilities::capabilities_for;
use crate::vista::source::SpacetimeTableShell;
use crate::vista::spec::{SpacetimeColumnExtras, SpacetimeTableExtras, SpacetimeVistaSpec};

pub struct SpacetimeVistaFactory {
    db: SpacetimeDb,
    /// Cached module schema, so building several tables costs one fetch.
    schema: Option<ModuleSchema>,
    /// Cached ownership answer. Writes need the database owner, not merely a
    /// token, and finding out costs a request — so ask once.
    can_write: Option<bool>,
}

impl SpacetimeVistaFactory {
    pub fn new(db: SpacetimeDb) -> Self {
        Self {
            db,
            schema: None,
            can_write: None,
        }
    }

    /// Read the module schema and the ownership answer once, and hold both.
    pub async fn load(mut self) -> Result<Self> {
        self.schema = Some(self.db.module_schema().await?);
        self.can_write = Some(self.db.can_write().await?);
        Ok(self)
    }

    pub fn schema(&self) -> Option<&ModuleSchema> {
        self.schema.as_ref()
    }

    /// Build a Vista over one table or view.
    ///
    /// Every property comes from the module definition: columns and types, the id
    /// column, and whether the relation is writable. Nothing needs declaring in
    /// YAML unless an author wants to narrow or re-label it.
    ///
    /// Fetches the schema on first use if [`Self::load`] was not called; after
    /// that it is [`Self::build`] with the network already done.
    pub async fn from_relation(&mut self, name: &str) -> Result<Vista> {
        if self.schema.is_none() {
            self.schema = Some(self.db.module_schema().await?);
        }
        if self.can_write.is_none() {
            // A failure here means we could not establish ownership, so assume
            // read-only: advertising writes we cannot perform is the worse error.
            self.can_write = Some(self.db.can_write().await.unwrap_or(false));
        }
        self.build(name)
    }

    /// Build a Vista over an already-loaded schema, without touching the network.
    ///
    /// This is the shape the rest of Vantage can actually call. `build_from_spec`
    /// is `fn(&self)` and a `ModelLoader` is `Fn() -> Result<Vista>` — both
    /// synchronous — so an `async fn(&mut self)` constructor, whatever else it
    /// is, cannot be registered in a catalog, reached from the CLI, or named in
    /// a UI inventory. Doing the two requests once in [`Self::load`] and leaving
    /// construction pure is what reconciles the two.
    pub fn build(&self, name: &str) -> Result<Vista> {
        let metadata = self.metadata_for(name)?;
        self.assemble(name, metadata)
    }

    /// The metadata the module declares for a relation, before any narrowing.
    fn metadata_for(&self, name: &str) -> Result<VistaMetadata> {
        self.loaded(name)?.metadata_for(name)
    }

    fn loaded(&self, name: &str) -> Result<&ModuleSchema> {
        self.schema.as_ref().ok_or_else(|| {
            error!(
                "the module schema has not been read yet — call `load()` (or use \
                 `from_relation`, which fetches it) before building",
                relation = name.to_string()
            )
        })
    }

    /// Wrap a relation and its metadata in a Vista.
    ///
    /// Split from [`Self::build`] so a spec can narrow the metadata on the way
    /// through: a `Vista` exposes no way to replace it afterwards.
    fn assemble(&self, name: &str, metadata: VistaMetadata) -> Result<Vista> {
        let schema = self.loaded(name)?;
        let table = schema
            .tables
            .get(name)
            .ok_or_else(|| error!("unknown relation", relation = name.to_string()))?;

        let identity = table.row_identity();
        let capabilities = capabilities_for(
            table.kind,
            self.can_write.unwrap_or(false),
            identity.column().is_some(),
        );
        Ok(Vista::new(
            name,
            SpacetimeTableShell::new(
                self.db.clone(),
                name.to_string(),
                metadata,
                identity,
                table.row_product_type(),
                capabilities,
            ),
        ))
    }
}

/// Building from YAML.
///
/// Deliberately thin. The module already declares columns, types, keys and
/// whether a relation is a table or a view, so a spec that restated them would
/// be a second source of truth that drifts the moment someone republishes. What
/// the spec contributes is the *name* — which relation, called what.
///
/// Anything it declares that this driver cannot lower is refused rather than
/// ignored. A silently dropped `computed:` block is a column that simply is not
/// there, with nothing to say why.
impl VistaFactory for SpacetimeVistaFactory {
    type TableExtras = SpacetimeTableExtras;
    type ColumnExtras = SpacetimeColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: SpacetimeVistaSpec) -> Result<Vista> {
        let relation = spec.driver.relation.as_deref().unwrap_or(&spec.name);

        if !spec.references.is_empty() {
            return Err(error!(
                "a SpacetimeDB module declares no foreign keys, so references cannot be \
                 resolved against it",
                relation = relation.to_string()
            )
            .mark_unsupported());
        }
        if !spec.contained.is_empty() {
            return Err(error!(
                "contained relations are not lowered by this driver — a nested SATS product \
                 reaches a Vista as one `json` column",
                relation = relation.to_string()
            )
            .mark_unsupported());
        }
        for (name, column) in &spec.columns {
            if column.lazy.is_some() || column.expr.is_some() {
                return Err(error!(
                    "computed and lazy columns need an expression vocabulary this driver does \
                     not have",
                    relation = relation.to_string(),
                    column = name.to_string()
                )
                .mark_unsupported());
            }
            if column.references.is_some() {
                return Err(error!(
                    "a column cannot declare a reference: the module has no foreign keys",
                    relation = relation.to_string(),
                    column = name.to_string()
                )
                .mark_unsupported());
            }
        }

        let declared_by_module = self.metadata_for(relation)?;

        // The module's key is authoritative. An override that disagrees would
        // address rows by a column the database does not key that way, so say so
        // rather than quietly honouring one or the other.
        if let Some(declared) = &spec.id_column
            && declared_by_module.id_column.as_deref() != Some(declared.as_str())
        {
            return Err(error!(
                "the spec's id_column is not the relation's key",
                relation = relation.to_string(),
                declared = declared.clone(),
                actual = declared_by_module
                    .id_column
                    .clone()
                    .unwrap_or_else(|| "none".into())
            ));
        }

        // A spec that lists columns is narrowing the view, which is the one
        // genuinely useful thing it can add. Every name has to exist in the
        // module — a typo that silently showed nothing would read as an empty
        // table, and the author would go looking at the database.
        let metadata = if spec.columns.is_empty() {
            declared_by_module
        } else {
            let mut narrowed = VistaMetadata::new();
            narrowed.id_column = declared_by_module.id_column.clone();
            for name in spec.columns.keys() {
                let column = declared_by_module.columns.get(name).ok_or_else(|| {
                    error!(
                        "the spec names a column the module does not declare",
                        relation = relation.to_string(),
                        column = name.clone()
                    )
                })?;
                narrowed = narrowed.with_column(column.clone());
            }
            narrowed
        };

        Ok(self.assemble(relation, metadata)?.with_name(spec.name))
    }
}
