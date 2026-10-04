//! `AwsVistaFactory` — typed-table entry point plus the `VistaFactory`
//! trait impl. AWS is read-only, so the factory advertises only
//! `can_count`. YAML construction (`build_from_spec`) is stubbed —
//! AWS table names encode wire-protocol details that don't lower
//! cleanly to a generic YAML schema, and no consumer needs it yet.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_table::table::{Table, VistaMetadataOptions};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{NoExtras, Vista, VistaCapabilities, VistaFactory, VistaMetadata};

use crate::AwsAccount;
use crate::vista::source::AwsTableShell;
use crate::vista::spec::{AwsColumnExtras, AwsTableExtras, AwsVistaSpec};

pub struct AwsVistaFactory {
    aws: AwsAccount,
}

impl AwsVistaFactory {
    pub fn new(aws: AwsAccount) -> Self {
        Self { aws }
    }

    /// Wrap a typed `Table<AwsAccount, E>` as a `Vista`. Column
    /// metadata, id field, and title fields are harvested from the
    /// typed table; the original `E` is erased to `EmptyEntity` for
    /// storage in the shell.
    pub fn from_table<E>(&self, table: Table<AwsAccount, E>) -> Result<Vista>
    where
        E: Entity<CborValue> + 'static,
    {
        let name = table.table_name().to_string();
        let any_table = table.into_entity::<EmptyEntity>();
        Ok(self.wrap(any_table, name))
    }

    fn wrap(&self, table: Table<AwsAccount, EmptyEntity>, name: String) -> Vista {
        let metadata = metadata_from_table(&table);
        // REST-XML (S3) listings can page one request at a time via
        // `start-after` — see [`AwsTableShell::fetch_next`].
        let can_fetch_next = crate::dispatch::parse_table_name(&name)
            .map(|op| op.protocol == crate::dispatch::Protocol::RestXml)
            .unwrap_or(false);
        let source = AwsTableShell::new(
            table,
            VistaCapabilities {
                can_count: true,
                can_fetch_next,
                ..VistaCapabilities::default()
            },
            metadata,
        );
        Vista::new(name, Box::new(source))
    }

    /// Borrow the underlying account — useful when callers need to
    /// build sibling tables off the same credentials.
    pub fn aws(&self) -> &AwsAccount {
        &self.aws
    }
}

impl VistaFactory for AwsVistaFactory {
    type TableExtras = AwsTableExtras;
    type ColumnExtras = AwsColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, _spec: AwsVistaSpec) -> Result<Vista> {
        Err(error!(
            "AWS Vista YAML spec construction is not implemented — \
             build a typed table via the constructors in `vantage_aws::models` \
             and wrap it with `AwsVistaFactory::from_table`"
        ))
    }
}

pub(crate) fn metadata_from_table<E>(table: &Table<AwsAccount, E>) -> VistaMetadata
where
    E: Entity<CborValue> + 'static,
{
    table.vista_metadata(VistaMetadataOptions {
        id_flag: true,
        ..VistaMetadataOptions::default()
    })
}
