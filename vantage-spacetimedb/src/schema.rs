//! Module-definition introspection: a SpacetimeDB `RawModuleDef` in, a
//! [`VistaMetadata`] out.
//!
//! SpacetimeDB is fully self-describing — `GET /v1/database/:db/schema` returns
//! every table, view, column, index, unique constraint and reducer signature —
//! so a Vantage table needs no column declarations in YAML unless the author
//! wants to narrow or re-label them.
//!
//! # Two schema versions, one normalised shape
//!
//! The host serves two module-definition ABIs and they differ by more than a
//! number:
//!
//! | | `?version=9` | `?version=10` |
//! |---|---|---|
//! | shape | flat: `tables[]`, `reducers[]`, `misc_exports[]` | `{ sections: [...] }` |
//! | table name field | `name` | `source_name` |
//! | views | inside `misc_exports` | a dedicated `Views` section |
//! | `is_event` | **absent** | present |
//!
//! We prefer **v10**, because event-ness is invisible in v9 and an event table
//! silently breaks a grid (see [`TableKind::Event`]). v9 remains as a fallback
//! for older hosts, with the loss recorded in
//! [`ModuleSchema::event_detection`] rather than papered over.
//!
//! Both are decoded through [`spacetimedb_sats::serde`]: the raw-def types
//! derive `SpacetimeType` (SATS), *not* serde, so plain
//! `serde_json::from_str::<RawModuleDefV9>` does not compile. SATS-JSON also
//! encodes `Option` as `{"some": x}` / `{"none": []}` and unit enum variants as
//! `{"Public": []}`, which serde's derive would not produce — another reason to
//! route through the official bridge instead of hand-written structs.
//!
//! # Columns live in the typespace
//!
//! A table definition does not carry its columns. It carries a
//! `product_type_ref` — an index into the module's `Typespace` — and the
//! `ProductType` found there holds the named elements. Primary keys and unique
//! constraints are likewise expressed as **column indices**, so every lookup
//! here resolves index → element name.

use indexmap::IndexMap;
use spacetimedb_lib::db::raw_def::v9::{RawModuleDefV9, TableAccess};
use spacetimedb_lib::db::raw_def::v10::{RawModuleDefV10, RawModuleDefV10Section};
use spacetimedb_lib::db::view::extract_view_return_product_type_ref;
use spacetimedb_sats::{AlgebraicType, AlgebraicTypeRef, ProductType, Typespace};
use vantage_core::{Result, error};
use vantage_vista::{Column as VistaColumn, VistaMetadata, flags};

/// Which module-definition ABI a [`ModuleSchema`] was decoded from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaVersion {
    /// Flat `RawModuleDefV9`. Cannot report event tables.
    V9,
    /// Section-based `RawModuleDefV10`. Reports `is_event`.
    V10,
}

impl SchemaVersion {
    /// The `?version=` query value this variant is fetched with.
    pub fn query_value(self) -> u32 {
        match self {
            SchemaVersion::V9 => 9,
            SchemaVersion::V10 => 10,
        }
    }

    /// Whether this ABI can tell us that a table is an event table.
    pub fn detects_event_tables(self) -> bool {
        matches!(self, SchemaVersion::V10)
    }
}

/// Whether a relation is a stored table, a server-side view, or an event table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableKind {
    /// An ordinary stored table. Readable, and writable with a token.
    Table,
    /// A server-defined view — a subscribable projection, possibly scoped to the
    /// calling identity. Read-only: there is nothing to write *to*.
    View,
    /// An event table. Rows are inserted and deleted within the same
    /// transaction — they exist only long enough to be broadcast, and never
    /// land in any cache.
    ///
    /// This is a trap for a Vantage grid, twice over: `list_vista_values` always
    /// returns nothing, and the insert/delete pair inside one transaction would
    /// be mistaken for an update by the delete/insert pairing in `watch_vista`.
    /// So the factory refuses these outright instead of showing a permanently
    /// empty table.
    Event,
}

/// One relation, normalised across schema versions.
#[derive(Debug, Clone)]
pub struct TableSchema {
    pub name: String,
    pub kind: TableKind,
    /// `true` when clients may read it at all. Private tables are reducer- and
    /// view-only; a client sees nothing, so a Vista over one is pointless.
    pub public: bool,
    /// Columns in declaration order, paired with their resolved SATS type.
    pub columns: Vec<(String, AlgebraicType)>,
    /// Primary-key column name, if the table declares exactly one. SpacetimeDB
    /// permits a zero-length primary key (meaning none) and does not yet support
    /// composite keys.
    pub primary_key: Option<String>,
    /// Single-column unique constraints, in declaration order. Used as the
    /// fallback identity when there is no primary key.
    pub unique_columns: Vec<String>,
    /// Views only: how many arguments the view function takes.
    ///
    /// A parameterised view cannot back a Vista yet — there is no channel for
    /// supplying the arguments — so [`ModuleSchema::metadata_for`] refuses one
    /// with a message saying so, rather than failing obscurely at query time.
    pub view_param_count: usize,
    /// Views only: `true` when the view takes an `AnonymousViewContext` and so
    /// cannot see who called it.
    ///
    /// Worth surfacing because the cost profile differs sharply: an
    /// identity-scoped view is computed and change-tracked **once per
    /// subscriber**, an anonymous one once in total. With many concurrent
    /// clients that is the difference between cheap and quadratic.
    pub view_is_anonymous: bool,
}

impl TableSchema {
    /// The row's SATS type, rebuilt from the resolved columns.
    ///
    /// The change feed needs this: pushed rows are bare BSATN with no
    /// self-description, so the schema is the only thing that can decode them.
    pub fn row_product_type(&self) -> ProductType {
        ProductType::new(
            self.columns
                .iter()
                .map(|(name, ty)| {
                    spacetimedb_sats::ProductTypeElement::new(ty.clone(), Some(name.clone().into()))
                })
                .collect(),
        )
    }

    /// Resolve the column whose value identifies a row, following the rule
    /// documented on [`RowIdentity`].
    pub fn row_identity(&self) -> RowIdentity {
        if let Some(pk) = &self.primary_key {
            return RowIdentity::PrimaryKey(pk.clone());
        }
        if let Some(unique) = self.unique_columns.first() {
            return RowIdentity::Unique(unique.clone());
        }
        RowIdentity::ContentHash
    }
}

/// How this driver derives Vista's `String` id for a row.
///
/// Vista fixes `Id = String`, so every backend must name one. SpacetimeDB
/// tables are not obliged to have a primary key, hence three cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowIdentity {
    /// The declared primary key, stringified.
    PrimaryKey(String),
    /// No primary key, but a single-column unique constraint — equally stable.
    Unique(String),
    /// Neither. The id is a stable hash of the row's own contents.
    ///
    /// Sound here in a way it would not be for most backends: SpacetimeDB
    /// deletes carry the **whole row**, not just a key, so a delete hashes to
    /// exactly the same id as the insert that created it. Pairing still works.
    ContentHash,
}

impl RowIdentity {
    /// The column backing this identity, or `None` for [`Self::ContentHash`].
    pub fn column(&self) -> Option<&str> {
        match self {
            RowIdentity::PrimaryKey(c) | RowIdentity::Unique(c) => Some(c),
            RowIdentity::ContentHash => None,
        }
    }
}

/// A whole module's schema, normalised and version-tagged.
#[derive(Debug, Clone)]
pub struct ModuleSchema {
    /// Which ABI this was decoded from.
    pub version: SchemaVersion,
    /// Relations by name — tables, views and event tables alike. Event tables
    /// are kept rather than dropped so the factory can refuse them with a
    /// specific message instead of "no such table".
    pub tables: IndexMap<String, TableSchema>,
    /// Reducer names in declaration order. Enough to validate a `call_reducer`
    /// target before issuing the request.
    pub reducers: Vec<String>,
}

impl ModuleSchema {
    /// Whether event tables can be identified at all — `false` on a v9 host,
    /// where the flag does not exist in the ABI.
    ///
    /// Callers should treat `false` as "an event table here would look like an
    /// ordinary empty table", and say so in diagnostics rather than guessing.
    pub fn event_detection(&self) -> bool {
        self.version.detects_event_tables()
    }

    /// Decode a `?version=10` response.
    pub fn from_v10_json(json: &str) -> Result<Self> {
        let module: RawModuleDefV10 = from_sats_json(json, "v10")?;

        let mut typespace: Option<Typespace> = None;
        let mut tables = IndexMap::new();
        let mut reducers = Vec::new();
        // Sections may appear in any order, so nothing can be resolved against
        // the typespace until the whole list has been walked.
        let mut raw_views = Vec::new();
        let mut raw_tables = Vec::new();

        for section in &module.sections {
            match section {
                RawModuleDefV10Section::Typespace(ts) => typespace = Some(ts.clone()),
                RawModuleDefV10Section::Tables(ts) => raw_tables.extend(ts.iter().cloned()),
                RawModuleDefV10Section::Views(vs) => raw_views.extend(vs.iter().cloned()),
                RawModuleDefV10Section::Reducers(rs) => {
                    reducers.extend(rs.iter().map(|r| r.source_name.to_string()));
                }
                // `RawModuleDefV10Section` is `#[non_exhaustive]` and gains
                // variants over time; anything we don't model is simply not
                // surfaced, which is the correct forward-compatible behaviour.
                _ => {}
            }
        }

        let typespace =
            typespace.ok_or_else(|| error!("SpacetimeDB v10 schema has no Typespace section"))?;

        for table in raw_tables {
            let name = table.source_name.to_string();
            let columns = columns_from_ref(&typespace, table.product_type_ref, &name)?;
            let kind = if table.is_event {
                TableKind::Event
            } else {
                TableKind::Table
            };
            let schema = TableSchema {
                primary_key: single_col_name(&columns, table.primary_key.iter().map(|c| c.idx())),
                unique_columns: unique_col_names_v10(&table, &columns),
                public: matches!(table.table_access, TableAccess::Public),
                name: name.clone(),
                kind,
                columns,
                view_param_count: 0,
                view_is_anonymous: false,
            };
            tables.insert(name, schema);
        }

        // Views resolve last, once the typespace is certainly in hand. A view's
        // columns come from its *return* type, which the ABI wraps as `T`,
        // `Option<T>`, `Vec<T>` or a query-builder marker — unwrapped here by
        // spacetimedb-lib's own helper rather than by re-deriving the encoding.
        for view in raw_views {
            let name = view.source_name.to_string();
            let Some((ty_ref, _kind)) = extract_view_return_product_type_ref(&view.return_type)
            else {
                // A view whose return type we can't unwrap is a view we would
                // mis-render. Skip it: it stays absent rather than wrong, and
                // `metadata_for` then reports "no such table or view".
                continue;
            };
            let columns = columns_from_ref(&typespace, ty_ref, &name)?;
            tables.insert(
                name.clone(),
                TableSchema {
                    name,
                    kind: TableKind::View,
                    public: view.is_public,
                    columns,
                    // Views have no primary key of their own in this section; the
                    // ABI carries those separately in `ViewPrimaryKeys`, which we
                    // do not model yet, so identity falls back to a content hash.
                    primary_key: None,
                    unique_columns: Vec::new(),
                    view_param_count: view.params.elements.len(),
                    view_is_anonymous: view.is_anonymous,
                },
            );
        }

        Ok(Self {
            version: SchemaVersion::V10,
            tables,
            reducers,
        })
    }

    /// Decode a `?version=9` response. Kept for hosts that do not serve v10;
    /// event tables are indistinguishable from ordinary ones here.
    pub fn from_v9_json(json: &str) -> Result<Self> {
        let module: RawModuleDefV9 = from_sats_json(json, "v9")?;
        let typespace = module.typespace.clone();

        let mut tables = IndexMap::new();
        for table in &module.tables {
            let name = table.name.to_string();
            let columns = columns_from_ref(&typespace, table.product_type_ref, &name)?;
            let schema = TableSchema {
                primary_key: single_col_name(&columns, table.primary_key.iter().map(|c| c.idx())),
                unique_columns: unique_col_names_v9(table, &columns),
                public: matches!(table.table_access, TableAccess::Public),
                name: name.clone(),
                // v9 carries no `is_event`, so every table reads as ordinary.
                kind: TableKind::Table,
                columns,
                view_param_count: 0,
                view_is_anonymous: false,
            };
            tables.insert(name, schema);
        }

        Ok(Self {
            version: SchemaVersion::V9,
            tables,
            reducers: module.reducers.iter().map(|r| r.name.to_string()).collect(),
        })
    }

    /// Build [`VistaMetadata`] for one relation.
    ///
    /// Refuses event tables and private tables — both would produce a Vista that
    /// silently shows nothing, which is worse than an error naming the cause.
    pub fn metadata_for(&self, table: &str) -> Result<VistaMetadata> {
        let schema = self.tables.get(table).ok_or_else(|| {
            error!(
                "SpacetimeDB module has no table or view with this name",
                table = table,
                known = self.tables.keys().cloned().collect::<Vec<_>>().join(", ")
            )
        })?;

        match schema.kind {
            TableKind::Event => {
                return Err(error!(
                    "SpacetimeDB event tables cannot back a Vista: their rows are deleted in the \
                     same transaction that inserts them, so the table always reads as empty. \
                     Use a normal table if the rows need to be listed or reviewed.",
                    table = table
                )
                .mark_unsupported());
            }
            TableKind::View if schema.view_param_count > 0 => {
                return Err(error!(
                    "SpacetimeDB view takes arguments, and a Vista has no channel for supplying \
                     them yet. Use a parameterless view, or a table with conditions.",
                    table = table,
                    params = schema.view_param_count as i64
                )
                .mark_unsupported());
            }
            _ if !schema.public => {
                return Err(error!(
                    "SpacetimeDB relation is private, so clients cannot read it at all. Mark the \
                     table `public`, or expose its rows through a public view.",
                    table = table
                )
                .mark_unsupported());
            }
            _ => {}
        }

        let identity = schema.row_identity();
        let mut metadata = VistaMetadata::new();

        for (name, ty) in &schema.columns {
            // Every column is orderable: this driver sorts client-side over the
            // materialised set, because SpacetimeDB SQL has no `ORDER BY`, so
            // there is no per-column server constraint to respect.
            let mut column =
                VistaColumn::new(name.clone(), vista_type_name(ty)).with_flag(flags::ORDERABLE);
            if identity.column() == Some(name.as_str()) {
                column = column.with_flag(flags::ID);
            }
            metadata = metadata.with_column(column);
        }

        // A content-hash identity names no column, so `id_column` stays unset and
        // the shell supplies the hash itself.
        if let Some(column) = identity.column() {
            metadata = metadata.with_id_column(column.to_string());
        }

        // Nominate a display column so a grid has something to show as the row
        // label: the first string that is not the id. Purely a default — an
        // inventory author can override it.
        let title = schema
            .columns
            .iter()
            .find(|(name, ty)| {
                matches!(ty, AlgebraicType::String) && identity.column() != Some(name.as_str())
            })
            .map(|(name, _)| name.clone());
        if let Some(title) = title
            && let Some(column) = metadata.columns.get_mut(&title)
        {
            column.flags.push(flags::TITLE.to_string());
            column.flags.push(flags::SEARCHABLE.to_string());
        }

        Ok(metadata)
    }
}

/// Decode SATS-JSON into a type that derives `SpacetimeType`.
///
/// The raw-def types implement SATS's `Deserialize`, not serde's, so this goes
/// through `SerdeWrapper` — which implements `serde::Deserialize` for any
/// `T: sats::Deserialize`.
fn from_sats_json<T>(json: &str, label: &str) -> Result<T>
where
    for<'de> spacetimedb_sats::serde::SerdeWrapper<T>: serde::Deserialize<'de>,
{
    let wrapper: spacetimedb_sats::serde::SerdeWrapper<T> =
        serde_json::from_str(json).map_err(|e| {
            error!(
                "could not decode SpacetimeDB module schema",
                version = label,
                error = e.to_string()
            )
        })?;
    Ok(wrapper.0)
}

/// Resolve a `product_type_ref` into named columns.
fn columns_from_ref(
    typespace: &Typespace,
    ty_ref: AlgebraicTypeRef,
    table: &str,
) -> Result<Vec<(String, AlgebraicType)>> {
    let ty = typespace.get(ty_ref).ok_or_else(|| {
        error!(
            "SpacetimeDB table references a type outside the module typespace",
            table = table
        )
    })?;
    let product: &ProductType = ty.as_product().ok_or_else(|| {
        error!(
            "SpacetimeDB table's row type is not a product type",
            table = table
        )
    })?;

    product
        .elements
        .iter()
        .enumerate()
        .map(|(idx, element)| {
            // The ABI requires every table column to be named; an unnamed one
            // means a malformed module rather than something to paper over.
            let name = element.name().ok_or_else(|| {
                error!(
                    "SpacetimeDB table has an unnamed column",
                    table = table,
                    index = idx as i64
                )
            })?;
            Ok((name.to_string(), element.algebraic_type.clone()))
        })
        .collect()
}

/// Map a column-index iterator to a single column name, or `None` when the list
/// is empty or composite (SpacetimeDB supports neither composite primary keys
/// nor, for our purposes, composite identity).
fn single_col_name(
    columns: &[(String, AlgebraicType)],
    mut indices: impl Iterator<Item = usize>,
) -> Option<String> {
    let first = indices.next()?;
    if indices.next().is_some() {
        return None;
    }
    columns.get(first).map(|(name, _)| name.clone())
}

fn unique_col_names_v9(
    table: &spacetimedb_lib::db::raw_def::v9::RawTableDefV9,
    columns: &[(String, AlgebraicType)],
) -> Vec<String> {
    use spacetimedb_lib::db::raw_def::v9::RawConstraintDataV9;
    table
        .constraints
        .iter()
        .filter_map(|c| match &c.data {
            RawConstraintDataV9::Unique(u) => {
                single_col_name(columns, u.columns.iter().map(|c| c.idx()))
            }
            _ => None,
        })
        .collect()
}

/// v10 reuses v9's constraint payload verbatim — `RawConstraintDataV10` is a
/// private alias for it — so the match arm is identical to
/// [`unique_col_names_v9`]; only the surrounding table type differs.
fn unique_col_names_v10(
    table: &spacetimedb_lib::db::raw_def::v10::RawTableDefV10,
    columns: &[(String, AlgebraicType)],
) -> Vec<String> {
    use spacetimedb_lib::db::raw_def::v9::RawConstraintDataV9;
    table
        .constraints
        .iter()
        .filter_map(|c| match &c.data {
            RawConstraintDataV9::Unique(u) => {
                single_col_name(columns, u.columns.iter().map(|c| c.idx()))
            }
            _ => None,
        })
        .collect()
}

/// Map a SATS type onto the type string Vista carries on a column.
///
/// The special SATS products (`Identity`, `ConnectionId`, `Timestamp`, …) are
/// classified before the structural cases, since each is technically a product
/// but means something scalar to a UI. Integers wider than `i64` map to
/// `"string"` deliberately: they do not fit the numeric types a Vista consumer
/// can hold, and a silently truncated id is worse than a textual one.
fn vista_type_name(ty: &AlgebraicType) -> &'static str {
    if ty.is_identity() || ty.is_connection_id() || ty.is_uuid() {
        return "string";
    }
    if ty.is_timestamp() {
        return "datetime";
    }
    if ty.is_time_duration() {
        return "duration";
    }
    if ty.is_bytes() {
        return "bytes";
    }
    // `Option<T>` renders as its inner type; nullability is a column property in
    // Vista, not a distinct type.
    if let Some(inner) = ty.as_option() {
        return vista_type_name(inner);
    }

    match ty {
        AlgebraicType::Bool => "bool",
        AlgebraicType::I8
        | AlgebraicType::U8
        | AlgebraicType::I16
        | AlgebraicType::U16
        | AlgebraicType::I32
        | AlgebraicType::U32
        | AlgebraicType::I64
        | AlgebraicType::U64 => "int",
        // Wider than i64 — see the note above.
        AlgebraicType::I128 | AlgebraicType::U128 | AlgebraicType::I256 | AlgebraicType::U256 => {
            "string"
        }
        AlgebraicType::F32 | AlgebraicType::F64 => "float",
        AlgebraicType::String => "string",
        // Nested structures round-trip as JSON text rather than being flattened;
        // a Vista column is scalar by contract.
        _ => "json",
    }
}
