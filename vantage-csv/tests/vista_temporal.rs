//! `vantage_vista::contract::check_temporal` over a CSV file per table, with `d`, `t` and `ts`
//! declared `date`, `time` and `timestamp`.

#![cfg(feature = "vista")]

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_csv::Csv;
use vantage_types::temporal;
use vantage_vista::contract::{Foreign, Stored, TemporalFixture, check_temporal};
use vantage_vista::{Vista, VistaFactory};

static TABLES: AtomicUsize = AtomicUsize::new(0);

/// Keeps the last table's file, so `stored` and `seed_foreign` reach past the vista.
struct Fixture {
    file: Mutex<Option<PathBuf>>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            file: Mutex::new(None),
        }
    }

    fn file(&self) -> PathBuf {
        self.file
            .lock()
            .unwrap()
            .clone()
            .expect("fresh() ran first")
    }
}

#[async_trait]
impl TemporalFixture for Fixture {
    async fn fresh(&self) -> Vista {
        let dir = std::env::temp_dir().join(format!(
            "vantage-csv-temporal-{}-{}",
            std::process::id(),
            TABLES.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("event.csv");
        std::fs::write(&file, "id,d,t,ts\n").unwrap();
        *self.file.lock().unwrap() = Some(file.clone());
        let yaml = format!(
            r#"
name: event
columns:
  id: {{ type: string, flags: [id] }}
  d: {{ type: date }}
  t: {{ type: time }}
  ts: {{ type: timestamp }}
csv:
  path: {}
"#,
            file.display()
        );
        Csv::new(dir).vista_factory().from_yaml(&yaml).unwrap()
    }

    async fn stored(&self, _vista: &Vista, id: &str, column: &str) -> Result<Stored> {
        let file = self.file();
        let mut reader = csv::Reader::from_path(&file).map_err(|e| error!(e.to_string()))?;
        let headers = reader.headers().map_err(|e| error!(e.to_string()))?.clone();
        let at = |name: &str| {
            headers
                .iter()
                .position(|h| h == name)
                .ok_or_else(|| error!(format!("no column {name} in {}", file.display())))
        };
        let (id_at, column_at) = (at("id")?, at(column)?);
        for row in reader.records() {
            let row = row.map_err(|e| error!(e.to_string()))?;
            if row.get(id_at) == Some(id) {
                return Ok(Stored::Text(
                    row.get(column_at).unwrap_or_default().to_string(),
                ));
            }
        }
        Err(error!(format!("row {id} is not in {}", file.display())))
    }

    async fn seed_foreign(&self, _vista: &Vista) -> Result<Vec<Foreign>> {
        let instant = |s: &str| tagged(temporal::TAG_INSTANT, s);
        let foreign = [
            (
                "ts",
                "2026-11-19T16:01:00Z",
                instant("2026-11-19T16:01:00.000000Z"),
            ),
            (
                "ts",
                "2026-11-19 16:01:00",
                instant("2026-11-19T16:01:00.000000Z"),
            ),
            (
                "ts",
                "2026-11-19 16:01:00.5",
                instant("2026-11-19T16:01:00.500000Z"),
            ),
            ("d", "2026-11-19", tagged(temporal::TAG_DATE, "2026-11-19")),
            ("t", "16:01", tagged(temporal::TAG_TIME, "16:01:00.000000")),
        ];
        let mut out = std::fs::OpenOptions::new()
            .append(true)
            .open(self.file())
            .map_err(|e| error!(e.to_string()))?;
        let mut seeded = Vec::new();
        for (i, (column, written, reads_as)) in foreign.into_iter().enumerate() {
            let cells: Vec<&str> = ["d", "t", "ts"]
                .iter()
                .map(|c| if *c == column { written } else { "" })
                .collect();
            writeln!(out, "f{i},{}", cells.join(",")).map_err(|e| error!(e.to_string()))?;
            seeded.push(Foreign::new(column, written, reads_as));
        }
        Ok(seeded)
    }
}

fn tagged(tag: u64, s: &str) -> CborValue {
    CborValue::Tag(tag, Box::new(CborValue::Text(s.to_string())))
}

#[tokio::test]
async fn csv_temporal_contract() {
    check_temporal(&Fixture::new()).await;
}
