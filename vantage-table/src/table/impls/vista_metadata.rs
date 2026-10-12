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
/// types and the `hidden`, `id` and `title` designations.
#[derive(Debug, Clone, Copy, Default)]
pub struct VistaMetadataOptions {
    pub orderable: Orderable,
    /// Flag computed columns (imported, expression, lazy) `calculated`.
    pub calculated: bool,
    /// Carry each column's `Searchable` and `Mandatory` flags over.
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
        metadata
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
        let col_flags = col.flags();
        if col_flags.contains(&TableColumnFlag::Hidden) {
            vc = vc.with_flag(ColumnFlag::Hidden);
        }
        if options.column_flags {
            if col_flags.contains(&TableColumnFlag::Searchable) {
                vc = vc.with_flag(ColumnFlag::Searchable);
            }
            if col_flags.contains(&TableColumnFlag::Mandatory) {
                vc = vc.with_flag(ColumnFlag::Mandatory);
            }
        }
        vc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::mock_table_source::MockTableSource;
    use vantage_types::EmptyEntity;

    fn table() -> Table<MockTableSource, EmptyEntity> {
        let mut table = Table::new("users", MockTableSource::new())
            .with_id_column("id")
            .with_title_column_of::<String>("name");
        table.add_computed_column(VistaColumn::new("shout", "string"));
        table
    }

    fn flags_of(metadata: &VistaMetadata, column: &str) -> Vec<ColumnFlag> {
        metadata.columns[column].flags().to_vec()
    }

    #[test]
    fn defaults_mark_only_the_id_and_titles() {
        let metadata = table().vista_metadata(VistaMetadataOptions::default());
        assert_eq!(metadata.id_column.as_deref(), Some("id"));
        assert!(flags_of(&metadata, "id").is_empty());
        assert_eq!(flags_of(&metadata, "name"), [ColumnFlag::Title]);
        let names: Vec<&String> = metadata.columns.keys().collect();
        assert_eq!(names, ["id", "name", "shout"]);
    }

    #[test]
    fn options_add_orderable_id_and_searchable_title_flags() {
        let metadata = table().vista_metadata(VistaMetadataOptions {
            orderable: Orderable::All,
            id_flag: true,
            searchable_titles: true,
            ..VistaMetadataOptions::default()
        });
        assert_eq!(
            flags_of(&metadata, "id"),
            [ColumnFlag::Orderable, ColumnFlag::Id]
        );
        assert_eq!(
            flags_of(&metadata, "name"),
            [
                ColumnFlag::Orderable,
                ColumnFlag::Title,
                ColumnFlag::Searchable
            ]
        );
        assert!(flags_of(&metadata, "shout").is_empty());
    }

    #[test]
    fn auto_id_flags_the_id_column() {
        let metadata = table()
            .with_auto_id()
            .vista_metadata(VistaMetadataOptions::default());
        assert!(metadata.columns["id"].has_flag(ColumnFlag::Auto));
    }
}
