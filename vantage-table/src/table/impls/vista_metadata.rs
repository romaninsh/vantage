//! Describing a table to Vista: the column, id, title, computed-column,
//! relation and contained-relation metadata a driver factory hands to its
//! shell.

use vantage_types::Entity;
use vantage_vista::{Column as VistaColumn, ColumnFlag, VistaMetadata};

use crate::{
    column::flags::ColumnFlag as TableColumnFlag,
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

/// Which columns [`Table::vista_metadata`] flags `orderable`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Orderable {
    #[default]
    None,
    All,
    /// Every column but imported implicit-reference ones (`author.name`),
    /// which exist only as a projection alias and can't be ordered by name.
    ExceptImported,
}

/// What a driver's [`Table::vista_metadata`] derives beyond column names,
/// types, the id and title designations, and the `hidden`, `label` and
/// `request_param` flags, which are always carried.
#[derive(Debug, Clone, Copy, Default)]
pub struct VistaMetadataOptions {
    pub orderable: Orderable,
    /// Flag computed columns (imported, expression, lazy) `calculated`.
    pub calculated: bool,
    /// Carry every column flag over through
    /// [`ColumnFlag::to_vista`](TableColumnFlag::to_vista). Without it only
    /// `Hidden`, `Label` and `RequestParam` are carried: they say how the
    /// column is shown or sent, which nothing else in the metadata derives.
    pub column_flags: bool,
    /// Flag the id column `id`.
    pub id_flag: bool,
    /// Flag title columns `searchable` as well.
    pub searchable_titles: bool,
    /// Fold in [`Table::vista_references`].
    pub references: bool,
    /// Fold in [`Table::vista_contained`].
    pub contained: bool,
}

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// Vista metadata for this table. Computed columns go in after the
    /// flags: the Vista holds their values and their spec carries their flags.
    /// The table's denials always go along, with expression and lazy columns
    /// denied as `"expression column"`, so the Vista refuses what the table
    /// would.
    pub fn vista_metadata(&self, options: VistaMetadataOptions) -> VistaMetadata {
        let id = self.id_field().map(|f| f.name().to_string());
        let titles = self.title_fields();
        let mut metadata = VistaMetadata::new();
        for (name, col) in self.columns() {
            let mut vc = self.vista_column(name, col, &options);
            if id.as_deref() == Some(name.as_str()) {
                if options.id_flag {
                    vc = vc.with_flag(ColumnFlag::Id);
                }
                if self.id_auto {
                    vc = vc.with_flag(ColumnFlag::Auto);
                }
            }
            if titles.iter().any(|t| t == name) {
                vc = vc.with_flag(ColumnFlag::Title);
                if options.searchable_titles {
                    vc = vc.with_flag(ColumnFlag::Searchable);
                }
            }
            metadata = metadata.with_column(vc);
        }
        if let Some(id) = id {
            metadata = metadata.with_id_column(id);
        }
        metadata = metadata.with_columns_at(self.computed_columns());
        if options.references {
            for reference in self.vista_references() {
                metadata = metadata.with_reference(reference);
            }
        }
        if options.contained {
            for reference in self.vista_contained() {
                metadata = metadata.with_reference(reference);
            }
        }
        let mut denials = self.denials().clone();
        for name in self.expressions.keys().chain(self.lazy_expressions.keys()) {
            denials.deny_column(name.as_str(), "expression column");
        }
        metadata.with_denials(denials)
    }

    fn vista_column(
        &self,
        name: &str,
        col: &T::Column<T::AnyType>,
        options: &VistaMetadataOptions,
    ) -> VistaColumn {
        let mut vc = VistaColumn::new(name, col.get_type());
        let orderable = match options.orderable {
            Orderable::None => false,
            Orderable::All => true,
            Orderable::ExceptImported => !self.is_imported_column(name),
        };
        if orderable {
            vc = vc.with_flag(ColumnFlag::Orderable);
        }
        if options.calculated && self.is_calculated_column(name) {
            vc = vc.with_flag(ColumnFlag::Calculated);
        }
        // A column's flags are a set; listing them by name keeps the Vista
        // column's flags in a stable order.
        let mut flags: Vec<ColumnFlag> = col
            .flags()
            .iter()
            .filter(|f| options.column_flags || describes_presentation(f))
            .map(TableColumnFlag::to_vista)
            .collect();
        flags.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        flags.into_iter().fold(vc, VistaColumn::with_flag)
    }

    /// This table's columns as Vista columns: name, declared type, and the
    /// `Hidden`, `Label` and `RequestParam` flags. Imported implicit-reference
    /// columns are flagged `calculated`, so readers render them read-only and
    /// keep them out of write payloads. Gives a contained sub-Vista its schema.
    pub fn vista_columns(&self) -> Vec<VistaColumn> {
        let options = VistaMetadataOptions::default();
        self.columns()
            .iter()
            .map(|(name, col)| {
                let vc = self.vista_column(name, col, &options);
                if self.is_imported_column(name) {
                    vc.with_flag(ColumnFlag::Calculated)
                } else {
                    vc
                }
            })
            .collect()
    }
}

/// Whether `flag` says how a column is shown or sent, carried into Vista
/// metadata even without [`VistaMetadataOptions::column_flags`].
fn describes_presentation(flag: &TableColumnFlag) -> bool {
    matches!(
        flag,
        TableColumnFlag::Hidden | TableColumnFlag::Label | TableColumnFlag::RequestParam
    )
}
