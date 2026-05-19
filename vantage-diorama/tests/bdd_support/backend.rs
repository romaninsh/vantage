//! Backend dispatch — turn a parsed Gherkin data table into a master `Vista`
//! against whichever backend the scenario selects.

use std::fs::File;
use std::io::Write as _;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_csv::Csv;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

use super::world::DioramaWorld;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackendKind {
    #[default]
    Mock,
    Csv,
    Sqlite,
}

impl BackendKind {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "csv" => Self::Csv,
            "sqlite" => Self::Sqlite,
            _ => Self::Mock,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RowSpec {
    pub id: String,
    pub fields: IndexMap<String, CborValue>,
}

#[derive(Clone, Debug, Default)]
pub struct MasterRows {
    pub name: String,
    pub id_column: String,
    pub columns: Vec<String>,
    pub rows: Vec<RowSpec>,
}

impl MasterRows {
    /// Build a synthetic `items` table with `count` rows. Ids are
    /// zero-padded so cache iteration order (redb is btree-ordered by
    /// key) matches index order. Each row has a single `title`
    /// column whose value is `"row-{i}"`.
    pub fn synthetic(count: usize) -> Self {
        let mut rows = Vec::with_capacity(count);
        for i in 0..count {
            let id = format!("{i:06}");
            let mut fields = IndexMap::new();
            fields.insert("title".to_string(), CborValue::Text(format!("row-{i}")));
            rows.push(RowSpec { id, fields });
        }
        Self {
            name: "items".to_string(),
            id_column: "id".to_string(),
            columns: vec!["id".to_string(), "title".to_string()],
            rows,
        }
    }

    /// Parse a Gherkin data table. The first row is the header; the
    /// `id_column` defaults to `"id"` and must appear in the header.
    pub fn from_table(name: &str, table: &cucumber::gherkin::Table) -> Self {
        let mut iter = table.rows.iter();
        let header = iter.next().cloned().unwrap_or_default();
        let id_column = "id".to_string();
        let id_idx = header
            .iter()
            .position(|c| c == &id_column)
            .expect("data table missing required `id` header");

        let rows = iter
            .map(|row| {
                let id = row[id_idx].clone();
                let mut fields = IndexMap::new();
                for (i, val) in row.iter().enumerate() {
                    if i == id_idx {
                        continue;
                    }
                    fields.insert(header[i].clone(), CborValue::Text(val.clone()));
                }
                RowSpec { id, fields }
            })
            .collect();

        Self {
            name: name.to_string(),
            id_column,
            columns: header,
            rows,
        }
    }

    /// Build a master `Vista` over the World's selected backend. The
    /// World owns whatever per-backend state must outlive the scenario
    /// (CSV temp directory, SQLite in-memory connection).
    pub async fn build_master_for(&self, w: &mut DioramaWorld) -> Result<Vista> {
        match w.backend {
            BackendKind::Mock => Ok(self.build_mock()),
            BackendKind::Csv => self.build_csv(w),
            BackendKind::Sqlite => self.build_sqlite(w).await,
        }
    }

    fn build_mock(&self) -> Vista {
        let mut meta = VistaMetadata::new().with_id_column(&self.id_column);
        for col in &self.columns {
            let mut c = Column::new(col, "String");
            if col == &self.id_column {
                c = c.with_flag("id");
            }
            meta = meta.with_column(c);
        }

        let mut shell = MockShell::new().with_metadata(meta);
        for row in &self.rows {
            let mut rec: Record<CborValue> = Record::new();
            for (k, v) in &row.fields {
                rec.insert(k.clone(), v.clone());
            }
            shell = shell.with_record(&row.id, rec);
        }
        Vista::new(self.name.clone(), Box::new(shell))
    }

    fn build_csv(&self, w: &mut DioramaWorld) -> Result<Vista> {
        let dir = w.tmp_path();
        let path = dir.join(format!("{}.csv", self.name));
        let mut f = File::create(&path).map_err(|e| {
            vantage_core::error!("failed to create CSV fixture", detail = e.to_string())
        })?;
        // Header row.
        writeln!(f, "{}", self.columns.join(",")).map_err(|e| {
            vantage_core::error!("failed to write CSV header", detail = e.to_string())
        })?;
        // Data rows. The fields map only carries non-id columns, so we
        // route through it for non-id columns and use row.id otherwise.
        for row in &self.rows {
            let mut cells: Vec<String> = Vec::with_capacity(self.columns.len());
            for col in &self.columns {
                if col == &self.id_column {
                    cells.push(row.id.clone());
                } else {
                    let val = row.fields.get(col).and_then(|v| match v {
                        CborValue::Text(s) => Some(s.clone()),
                        _ => None,
                    });
                    cells.push(val.unwrap_or_default());
                }
            }
            writeln!(f, "{}", cells.join(",")).map_err(|e| {
                vantage_core::error!("failed to write CSV row", detail = e.to_string())
            })?;
        }
        drop(f);

        let csv = Csv::new(&dir).with_id_column(&self.id_column);
        let mut table =
            Table::<Csv, EmptyEntity>::new(&self.name, csv.clone()).with_id_column(&self.id_column);
        for col in &self.columns {
            if col != &self.id_column {
                table = table.with_column_of::<String>(col);
            }
        }
        csv.vista_factory().from_table(table)
    }

    async fn build_sqlite(&self, w: &mut DioramaWorld) -> Result<Vista> {
        // Provision the pool + DDL + seed data on a dedicated thread with
        // its own real-time runtime. The main test runtime runs paused
        // virtual time (needed for refresh scenarios), which would deadlock
        // sqlx's internal acquire-timeout machinery. Once the DB is set up
        // and returned here, reads from the Vista's `Table` use the
        // pool's worker threads — they don't depend on the test runtime's
        // clock.
        let name = self.name.clone();
        let id_column = self.id_column.clone();
        let columns = self.columns.clone();
        let rows = self.rows.clone();
        let db = std::thread::spawn(move || -> Result<SqliteDB> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| {
                    vantage_core::error!("build provisioning runtime", detail = e.to_string())
                })?;
            rt.block_on(async move {
                let db = SqliteDB::connect("sqlite::memory:")
                    .await
                    .map_err(|e| vantage_core::error!("sqlite connect", detail = e.to_string()))?;

                let col_defs: Vec<String> = columns
                    .iter()
                    .map(|c| {
                        if c == &id_column {
                            format!("{c} TEXT PRIMARY KEY")
                        } else {
                            format!("{c} TEXT")
                        }
                    })
                    .collect();
                let create = format!("CREATE TABLE {name} ({})", col_defs.join(", "));
                sqlx::query(&create)
                    .execute(db.pool())
                    .await
                    .map_err(|e| vantage_core::error!("CREATE TABLE", detail = e.to_string()))?;

                for row in &rows {
                    let mut values: Vec<String> = Vec::with_capacity(columns.len());
                    for col in &columns {
                        let raw = if col == &id_column {
                            row.id.clone()
                        } else {
                            row.fields
                                .get(col)
                                .and_then(|v| match v {
                                    CborValue::Text(s) => Some(s.clone()),
                                    _ => None,
                                })
                                .unwrap_or_default()
                        };
                        values.push(format!("'{}'", raw.replace('\'', "''")));
                    }
                    let stmt = format!(
                        "INSERT INTO {name} ({}) VALUES ({})",
                        columns.join(", "),
                        values.join(", ")
                    );
                    sqlx::query(&stmt)
                        .execute(db.pool())
                        .await
                        .map_err(|e| vantage_core::error!("INSERT", detail = e.to_string()))?;
                }

                Ok(db)
            })
        })
        .join()
        .map_err(|_| vantage_core::error!("sqlite provisioning thread panicked"))??;

        let mut table = Table::<SqliteDB, EmptyEntity>::new(&self.name, db.clone())
            .with_id_column(&self.id_column);
        for col in &self.columns {
            if col != &self.id_column {
                table = table.with_column_of::<String>(col);
            }
        }
        // Stash the connection so it outlives the scenario. Dropping it
        // would close `sqlite::memory:` and the Vista's reads would fail.
        w.sqlite_db = Some(db.clone());

        vantage_sql::sqlite::vista::SqliteVistaFactory::new(db).from_table::<EmptyEntity>(table)
    }
}

// Silence the dead-code warning on AnySqliteType — pulled in for trait
// disambiguation but not named directly.
#[allow(dead_code)]
const _: Option<AnySqliteType> = None;
