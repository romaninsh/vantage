//! A bucket of 200 objects whose `name` and `size` come from a detail
//! source: more rows than one viewport, so a view or a read that judges an
//! augmented clause over what happens to be hydrated misses most of the set.
//!
//! Object `i` is `o{i:03}`, `kind` red for even `i` and blue for odd, `size`
//! `i`, and named John when `i % 10 == 7` (20 Johns, only two in the first
//! 20 rows), Jane otherwise.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_diorama::{Augmentation, Cache, Dio, Lens};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Capability, Column, TableShell, Vista, VistaCapabilities, VistaMetadata, forward_table_shell,
};

use crate::support::{augmented_dio, copy_master, fixed_augmentation, quick_lens};

pub const ROWS: usize = 200;

pub fn object_id(i: usize) -> String {
    format!("o{i:03}")
}

pub fn is_john(i: usize) -> bool {
    i % 10 == 7
}

/// The Johns' ids, in listing order.
pub fn john_ids() -> Vec<String> {
    (0..ROWS).filter(|i| is_john(*i)).map(object_id).collect()
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn row(pairs: Vec<(&str, CborValue)>) -> Record<CborValue> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// The master shell: `id` and `kind`, every capability the mock has.
pub fn master_shell() -> MockShell {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("kind", "String"))
        .with_id_column("id");
    (0..ROWS).fold(MockShell::new().with_metadata(metadata), |shell, i| {
        let kind = if i % 2 == 0 { "red" } else { "blue" };
        shell.with_record(
            object_id(i),
            row(vec![("id", text(&object_id(i))), ("kind", text(kind))]),
        )
    })
}

/// What the detail source does: which id it fails on, and how many reads it
/// has answered.
#[derive(Clone, Default)]
pub struct DetailProbe {
    pub fails: Option<String>,
    pub gets: Arc<AtomicUsize>,
}

impl DetailProbe {
    pub fn failing(id: &str) -> Self {
        Self {
            fails: Some(id.to_string()),
            ..Self::default()
        }
    }

    pub fn gets(&self) -> usize {
        self.gets.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
struct ProbedDetail {
    inner: Box<dyn TableShell>,
    probe: DetailProbe,
}

forward_table_shell!(ProbedDetail, inner, {
    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        self.probe.gets.fetch_add(1, Ordering::SeqCst);
        if self.probe.fails.as_deref() == Some(id) {
            return Err(vantage_core::error!(
                "detail read failed (injected)",
                id = id
            ));
        }
        self.inner.get(id).await
    }
});

/// The detail source: `name` and `size` by object id, read through `probe`.
fn detail(probe: DetailProbe) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("size", "Integer"))
        .with_id_column("id");
    let shell = (0..ROWS).fold(MockShell::new().with_metadata(metadata), |shell, i| {
        let name = if is_john(i) { "John" } else { "Jane" };
        shell.with_record(
            object_id(i),
            row(vec![
                ("id", text(&object_id(i))),
                ("name", text(name)),
                ("size", CborValue::Integer((i as i64).into())),
            ]),
        )
    });
    Vista::new("names", shell).map_shell(|inner| ProbedDetail { inner, probe })
}

pub fn augmentation() -> Augmentation {
    fixed_augmentation(detail(DetailProbe::default()), &["name", "size"])
}

/// A Dio over `master` with no loader: a two-pass view's list pass fills
/// its cache.
pub async fn listed_dio(master: Vista) -> Dio {
    probed_dio(master, DetailProbe::default(), &["name", "size"]).await
}

/// [`listed_dio`] reading its detail through `probe`, lifting `columns`
/// from it (none: a lift-all augmentation, which lifts every column).
pub async fn probed_dio(master: Vista, probe: DetailProbe, columns: &[&str]) -> Dio {
    let augmentation = fixed_augmentation(detail(probe), columns);
    augmented_dio(&quick_lens(Cache::memory()), master, vec![augmentation]).await
}

/// A Dio whose cache is copied whole from the master when it is built,
/// every row unhydrated.
pub async fn copied_dio() -> Result<Dio> {
    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(copy_master())
        .build()?;
    Ok(augmented_dio(
        &lens,
        Vista::new("bucket", master_shell()),
        vec![augmentation()],
    )
    .await)
}

/// A master shell that counts its listings and advertises `caps`.
#[derive(Clone)]
pub struct CountedLists {
    inner: Box<dyn TableShell>,
    caps: VistaCapabilities,
    lists: Arc<AtomicUsize>,
}

forward_table_shell!(CountedLists, inner, {
    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        self.lists.fetch_add(1, Ordering::SeqCst);
        self.inner.list().await
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.caps
    }
});

/// The bucket behind a master without `Window`, and its listing count.
pub fn without_window() -> (Vista, Arc<AtomicUsize>) {
    let lists = Arc::new(AtomicUsize::new(0));
    let caps: VistaCapabilities = [Capability::Order, Capability::Search, Capability::FilterOps]
        .into_iter()
        .collect();
    let counter = lists.clone();
    let master = Vista::new("bucket", master_shell()).map_shell(move |inner| CountedLists {
        inner,
        caps,
        lists: counter,
    });
    (master, lists)
}
