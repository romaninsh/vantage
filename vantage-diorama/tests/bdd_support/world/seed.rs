//! The warm cache a paged Lens's "on_start" seed leaves behind.

use std::sync::atomic::Ordering;

use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::Vista;

use super::config::OnStartLoad;
use super::{DioramaWorld, on_master};
use crate::bdd_support::backend::BackendKind;

impl DioramaWorld {
    /// The "on_start" seed of a paged Lens. A paged loader's eager start runs
    /// only for a master without `Window`, so the harness copies the start's
    /// rows into the Dio's cache table itself — the warm cache a paged view
    /// opens on — and stores a total provider's count as the remembered
    /// total.
    pub(super) async fn seed_paged_start(&mut self, master: &Vista) {
        let load_kind = self.lens_builder.load_kind();
        if !self.lens_builder.pages() || matches!(load_kind, OnStartLoad::Unset) {
            return;
        }
        // The seed writes the table named after the master, which is the
        // Dio's table only while the master carries no query.
        assert!(
            master.query().is_empty(),
            "a paged seed needs an unnarrowed master; a narrowed one keys its own cache table"
        );
        self.spies.on_start.fetch_add(1, Ordering::SeqCst);
        self.spies.master_list_calls.fetch_add(1, Ordering::SeqCst);
        let bridge = self.backend == BackendKind::Sqlite;
        let rows = on_master(
            bridge,
            master.clone(),
            |m| async move { m.list_values().await },
        )
        .await
        .expect("list master");
        let total = self
            .lens_builder
            .total_provider_kind
            .count(&self.spies, rows.len());
        let table = self
            .cache_source()
            .open_table(master.name())
            .await
            .expect("open cache table");
        table
            .insert_values(load_kind.select(rows))
            .await
            .expect("seed cache");
        if let Some(total) = total {
            table
                .set_meta_total(total as u64)
                .await
                .expect("remember the total");
        }
    }
}
