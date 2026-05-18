//! Backend dispatch — turn a parsed Gherkin data table into a master `Vista`
//! against whichever backend the scenario selects.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

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

    pub async fn build_master(&self, backend: BackendKind) -> Result<Vista> {
        match backend {
            BackendKind::Mock => Ok(self.build_mock()),
            BackendKind::Csv => unimplemented!("CSV backend lands in Phase 5"),
            BackendKind::Sqlite => unimplemented!("SQLite backend lands in Phase 5"),
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
}
