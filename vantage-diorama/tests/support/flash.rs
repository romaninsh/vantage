//! Fixtures for the flash pipeline tests: a seeded Dio, and waiting for a
//! flash to stage.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tokio::sync::broadcast;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Dio, DioEvent, Lens};
use vantage_types::Record;
use vantage_vista::Vista;

/// A Dio over `master` on `lens`, its cache holding the master's rows.
pub async fn seeded_dio(lens: &Arc<Lens>, master: Vista) -> Result<Dio> {
    let dio = lens.dio(master).build().await?;
    dio.reconcile(dio.master().list_values().await?).await?;
    Ok(dio)
}

pub async fn cached(dio: &Dio, id: &str) -> Result<Option<Record<CborValue>>> {
    dio.cached().get_value(id).await
}

/// Wait until the bus has announced a staged flash for each of `ids`.
pub async fn staged(bus: &mut broadcast::Receiver<DioEvent>, ids: &[&str]) {
    let mut waiting: Vec<&str> = ids.to_vec();
    let wait = async {
        while !waiting.is_empty() {
            if let Ok(DioEvent::WritePending { id, .. }) = bus.recv().await {
                waiting.retain(|w| *w != id);
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .expect("the flash was never staged");
}
