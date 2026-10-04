//! `KubeVistaFactory` — typed-table entry point plus the `VistaFactory`
//! trait impl. Kubernetes is read-only in v1, so the shell advertises only
//! `can_count`. YAML construction (`build_from_spec`) is stubbed until the
//! inventory app needs it.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_table::table::{Orderable, Table, VistaMetadataOptions};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{NoExtras, Vista, VistaCapabilities, VistaFactory, VistaMetadata};

use crate::cluster::KubernetesCluster;
use crate::vista::source::KubeTableShell;
use crate::vista::spec::{KubeColumnExtras, KubeTableExtras, KubeVistaSpec};

pub struct KubeVistaFactory {
    cluster: KubernetesCluster,
}

impl KubeVistaFactory {
    pub fn new(cluster: KubernetesCluster) -> Self {
        Self { cluster }
    }

    /// Wrap a typed `Table<KubernetesCluster, E>` as a `Vista`.
    pub fn from_table<E>(&self, table: Table<KubernetesCluster, E>) -> Result<Vista>
    where
        E: Entity<CborValue> + 'static,
    {
        let name = table.table_name().to_string();
        let any_table = table.into_entity::<EmptyEntity>();
        Ok(self.wrap(any_table, name))
    }

    fn wrap(&self, table: Table<KubernetesCluster, EmptyEntity>, name: String) -> Vista {
        let metadata = metadata_from_table(&table);
        let source = KubeTableShell::new(
            table,
            // All read capabilities are honoured client-side over the
            // materialised listing (see `KubeTableShell`). Writes stay off.
            VistaCapabilities {
                can_count: true,
                can_order: true,
                can_search: true,
                can_set_page_size: true,
                can_fetch_page: true,
                can_fetch_next: true,
                can_fetch_window: true,
                ..VistaCapabilities::default()
            },
            metadata,
        );
        Vista::new(name, Box::new(source))
    }

    /// Borrow the underlying cluster — useful to build sibling tables.
    pub fn cluster(&self) -> &KubernetesCluster {
        &self.cluster
    }
}

impl VistaFactory for KubeVistaFactory {
    type TableExtras = KubeTableExtras;
    type ColumnExtras = KubeColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, _spec: KubeVistaSpec) -> Result<Vista> {
        Err(error!(
            "Kubernetes Vista YAML spec construction is not implemented yet — \
             build a typed table via `vantage_kubernetes::models` and wrap it \
             with `KubeVistaFactory::from_table`"
        ))
    }
}

pub(crate) fn metadata_from_table<E>(table: &Table<KubernetesCluster, E>) -> VistaMetadata
where
    E: Entity<CborValue> + 'static,
{
    table.vista_metadata(VistaMetadataOptions {
        // The shell sorts client-side over the materialised listing, so
        // every column is sortable.
        orderable: Orderable::All,
        id_flag: true,
        // The quicksearch scans all text fields; flagging the title tells
        // UIs which one is primary.
        searchable_titles: true,
        ..VistaMetadataOptions::default()
    })
}
