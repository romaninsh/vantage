//! A live source Dio backed by a mutable in-memory master.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Dio, Generation, Lens};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

/// Debounce used across the suite. Short enough to keep tests quick, long
/// enough that a burst genuinely coalesces.
pub const DEBOUNCE: Duration = Duration::from_millis(40);

/// Long enough for a leading-edge recompute, the cooldown, and the trailing
/// flush to all land.
pub async fn settle() {
    tokio::time::sleep(DEBOUNCE * 5).await;
}

pub fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

pub fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

pub fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

/// A master whose rows a test can mutate mid-run.
pub fn master(columns: &[(&str, &str)]) -> MockShell {
    let mut metadata =
        VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id"));
    for (name, ty) in columns {
        metadata = metadata.with_column(Column::new(*name, *ty));
    }
    MockShell::new().with_metadata(metadata.with_id_column("id"))
}

/// An eager source Dio: `on_start` copies the master into the cache,
/// `on_refresh` replaces it. Mutating the shell then calling
/// [`Dio::refresh`] is how a test simulates the datasource changing.
pub async fn source(shell: MockShell) -> Dio {
    build_source(shell, None).await
}

/// Same as [`source`], but with the official debug stream turned on for
/// this datasource — the tap an aggregate spawned over this Dio inherits.
pub async fn source_with_debug(shell: MockShell, ds_name: &str) -> Dio {
    build_source(shell, Some(ds_name)).await
}

async fn build_source(shell: MockShell, debug: Option<&str>) -> Dio {
    let mut builder = Lens::new().cache_in_memory();
    if let Some(name) = debug {
        builder = builder.debug_datasource(name);
    }
    let lens = Arc::new(
        builder
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await?;
                    Ok(())
                }
            })
            .on_refresh(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().clear().await?;
                    dio.cache().insert_values(rows).await?;
                    Ok(())
                }
            })
            .build()
            .expect("source lens builds"),
    );
    let vista = Vista::new("items", Box::new(shell));
    lens.make_dio(vista).await.expect("source dio")
}

/// Counts generation bumps on a scenery's watch channel.
pub struct BumpCounter {
    rx: tokio::sync::watch::Receiver<Generation>,
    seen: u64,
    count: usize,
}

impl BumpCounter {
    pub fn new(rx: tokio::sync::watch::Receiver<Generation>) -> Self {
        let seen = u64::from(*rx.borrow());
        Self { rx, seen, count: 0 }
    }

    /// Bumps observed since the last call.
    pub fn drain(&mut self) -> usize {
        let current = u64::from(*self.rx.borrow_and_update());
        if current > self.seen {
            self.count += (current - self.seen) as usize;
            self.seen = current;
        }
        std::mem::take(&mut self.count)
    }
}
