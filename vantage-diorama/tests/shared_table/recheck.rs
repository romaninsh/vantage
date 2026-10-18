//! A Dio built with `recheck_facts` re-reads every fact through its own
//! master, for a rule its master applies but its query doesn't show.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use tokio::sync::Semaphore;
use vantage_core::Result;
use vantage_diorama::{ChangeFlash, Dio};
use vantage_types::Record;
use vantage_vista::{Page, TableShell, Vista, forward_table_shell};

use crate::fixtures::{cached_cell, holds, ids, invoice_master, patch, until, view_showing};
use crate::support::{copy_master, memory_lens, record, text};

/// A master that returns only client 7's rows, by a rule its query never
/// records (the way a table hook narrows one): every read filters, so
/// nothing a hub fact carries can be judged in memory. Counts its `get`s,
/// and holds each one at `gate` until it has a permit.
#[derive(Clone)]
struct HiddenRule {
    inner: Box<dyn TableShell>,
    gets: Arc<AtomicUsize>,
    gate: Arc<Semaphore>,
}

type Row = Record<CborValue>;

fn in_seven(row: &Row) -> bool {
    row.get("client_id") == Some(&text("7"))
}

impl HiddenRule {
    async fn sevens(&self) -> Result<IndexMap<String, Row>> {
        let mut rows = self.inner.list().await?;
        rows.retain(|_, row| in_seven(row));
        Ok(rows)
    }
}

forward_table_shell!(HiddenRule, inner, {
    async fn list(&self) -> Result<IndexMap<String, Row>> {
        self.sevens().await
    }
    async fn get(&self, id: &str) -> Result<Option<Row>> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        self.gate.acquire().await.expect("gate open").forget();
        Ok(self.inner.get(id).await?.filter(in_seven))
    }
    async fn count(&self) -> Result<i64> {
        Ok(self.sevens().await?.len() as i64)
    }
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        let rows = self.sevens().await?;
        let total = rows.len() as i64;
        let window = rows.into_iter().skip(offset).take(limit).collect();
        Ok(Page::new(window).with_total(total))
    }
});

/// A over the whole table, and B over the hidden rule, sharing a hub by key
/// (their vistas are named apart, so they keep a cache table each).
struct Hidden {
    a: Dio,
    b: Dio,
    gets: Arc<AtomicUsize>,
    gate: Arc<Semaphore>,
}

impl Hidden {
    /// B's master answers every `get` at once.
    async fn open() -> Result<Self> {
        Self::gated(Semaphore::MAX_PERMITS).await
    }

    /// B's master answers `permits` gets, then holds the rest at the gate.
    async fn gated(permits: usize) -> Result<Self> {
        let lens = memory_lens(copy_master());
        let shell = invoice_master();
        let gets = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Semaphore::new(permits));
        let a = lens
            .dio(Vista::new("invoice", shell.clone()))
            .hub("invoice")
            .build()
            .await?;
        let hidden = Vista::new("invoice for 7", shell).map_shell(|inner| HiddenRule {
            inner,
            gets: gets.clone(),
            gate: gate.clone(),
        });
        let b = lens
            .dio(hidden)
            .hub("invoice")
            .recheck_facts()
            .build()
            .await?;
        Ok(Self { a, b, gets, gate })
    }

    fn gets(&self) -> usize {
        self.gets.load(Ordering::SeqCst)
    }
}

fn insert(id: &str, client: &str) -> ChangeFlash {
    ChangeFlash::insert(id, record(&[("id", id), ("client_id", client)]))
}

#[tokio::test]
async fn recheck_dio_drops_a_row_its_master_no_longer_returns() -> Result<()> {
    let Hidden { a, b, .. } = Hidden::open().await?;
    let b_view = view_showing(&b, &["x"]).await;

    a.flash(patch("x", &[("client_id", "8")])).await?;

    until("B's view drops x", async || ids(&b_view).is_empty()).await;
    Ok(())
}

#[tokio::test]
async fn recheck_dio_admits_a_row_its_master_now_returns() -> Result<()> {
    let Hidden { a, b, .. } = Hidden::open().await?;
    let b_view = view_showing(&b, &["x"]).await;

    a.flash(insert("y", "7")).await?;

    until("B lists y", async || {
        ids(&b_view).contains(&"y".to_string())
    })
    .await;
    Ok(())
}

/// The security half: a row A writes outside B's hidden rule never reaches
/// B, though A publishes it and B re-reads it.
#[tokio::test]
async fn recheck_dio_never_admits_a_row_outside_its_hidden_rule() -> Result<()> {
    let Hidden { a, b, .. } = Hidden::open().await?;
    let b_view = view_showing(&b, &["x"]).await;

    a.flash(insert("w", "8")).await?;
    // The sentinel: a later row inside the rule, re-read after w.
    a.flash(insert("y", "7")).await?;

    until("B lists y", async || {
        ids(&b_view).contains(&"y".to_string())
    })
    .await;
    assert!(!holds(&b, "w").await);
    assert!(!ids(&b_view).contains(&"w".to_string()));
    Ok(())
}

/// B's first re-read of `x` waits at the gate while four more writes to
/// `x` and an insert of `y` queue behind it. Once the gate opens, the queued
/// facts cost one read of `x` and one of `y`.
#[tokio::test]
async fn recheck_coalesces_a_burst_into_one_read() -> Result<()> {
    let hidden = Hidden::gated(0).await?;
    let Hidden { a, b, .. } = &hidden;
    until("B holds x", async || holds(b, "x").await).await;
    let before = hidden.gets();

    a.flash(patch("x", &[("amount", "20")])).await?;
    until("B's re-read of x waits at the gate", async || {
        hidden.gets() == before + 1
    })
    .await;
    for amount in ["21", "22", "23", "24"] {
        a.flash(patch("x", &[("amount", amount)])).await?;
    }
    a.flash(insert("y", "7")).await?;
    hidden.gate.add_permits(10);

    until("B holds y", async || holds(b, "y").await).await;
    until("B holds the last amount", async || {
        cached_cell(b, "x", "amount").await.as_deref() == Some("24")
    })
    .await;
    assert_eq!(
        hidden.gets() - before,
        3,
        "the gated read of x, then one of x and one of y"
    );
    Ok(())
}
