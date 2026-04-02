use indexmap::IndexMap;
use std::collections::HashSet;
use std::ops::Index;
use vantage_expressions::{Expression, Expressive, expr};

impl<T: TableSource, E: Entity> Table<T, E>
where
    T::Column: ColumnLike,
{
    pub fn with_column(mut self, column: impl Into<T::Column>) -> Self {
        let column = column.into();
        let column_name = column.name().to_string();
        let flags = column.flags();

        // Auto-set id_field if column has IdField flag and id_field is not yet set
        if flags.contains(&ColumnFlag::IdField) && self.id_field.is_none() {
            self.id_field = Some(column_name.clone());
        }

        // Auto-set title_field if column has TitleField flag and title_field is not yet set
        if flags.contains(&ColumnFlag::TitleField) && self.title_field.is_none() {
            self.title_field = Some(column_name.clone());
        }

        self.columns.insert(column_name, column);
        self
    }

    /// Add a column to the table
    pub fn add_column(&mut self, column: impl Into<T::Column>) {
        let column = column.into();
        let column_name = column.name().to_string();
        let flags = column.flags();

        // Auto-set id_field if column has IdField flag and id_field is not yet set
        if flags.contains(&ColumnFlag::IdField) && self.id_field.is_none() {
            self.id_field = Some(column_name.clone());
        }

        // Auto-set title_field if column has TitleField flag and title_field is not yet set
        if flags.contains(&ColumnFlag::TitleField) && self.title_field.is_none() {
            self.title_field = Some(column_name.clone());
        }

        self.columns.insert(column_name, column);
    }

    /// Add an ID column to the table (typically String type for most databases)
    /// This is a convenience method for defining the primary key column
    pub fn with_id_column(self, name: impl Into<String>) -> Self
    where
        T::Column: From<Column>,
    {
        self.with_column(Column::new(name.into()).with_flag(ColumnFlag::IdField))
    }

    /// Add a title column to the table
    /// This is a convenience method for defining the display title/name column
    /// Title columns are used to describe a record when only a single value is possible,
    /// for example on confirmation dialogs
    pub fn with_title_column(self, name: impl Into<String>) -> Self
    where
        T::Column: From<Column>,
    {
        self.with_column(Column::new(name.into()).with_flag(ColumnFlag::TitleField))
    }

    /// Get all columns
    pub fn columns(&self) -> &IndexMap<String, T::Column> {
        &self.columns
    }

    /// Get a reference to a column for operations
    pub fn column(&self, name: &str) -> Option<&T::Column> {
        self.columns.get(name)
    }
}

impl<T: TableSource, E: Entity> Index<&str> for Table<T, E> {
    type Output = T::Column;

    fn index(&self, index: &str) -> &Self::Output {
        &self.columns[index]
    }
}

impl From<Column> for IntoExpressive<Expression> {
    fn from(val: Column) -> Self {
        IntoExpressive::nested(val.expr())
    }
}

impl From<&Column> for IntoExpressive<Expression> {
    fn from(val: &Column) -> Self {
        IntoExpressive::nested(val.expr())
    }
}

impl From<Column> for String {
    fn from(val: Column) -> Self {
        val.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::MockTableSource;
    use crate::with_ordering::OrderByExt;
    use vantage_expressions::expr;

    #[test]
    fn test_column_in_expression() {
        let datasource = MockTableSource::new();
        let table = Table::new("users", datasource)
            .with_column("is_vip")
            .with_column("name");

        let expr = expr!("{} = true", &table["is_vip"]);
        assert_eq!(expr.preview(), "is_vip = true");
    }

    #[test]
    fn test_column_with_flags() {
        let column = Column::new("email").with_flags(&[ColumnFlag::Mandatory]);
        assert!(column.flags().contains(&ColumnFlag::Mandatory));
        assert_eq!(column.flags().len(), 1);
    }

    #[test]
    fn test_column_with_flag() {
        let column = Column::new("id").with_flag(ColumnFlag::IdField);
        assert!(column.flags().contains(&ColumnFlag::IdField));
        assert_eq!(column.flags().len(), 1);
    }

    #[test]
    fn test_column_no_flags() {
        let column = Column::new("optional_field");
        assert!(column.flags().is_empty());
    }

    #[test]
    fn test_table_auto_set_id_field() {
        let datasource = MockTableSource::new();
        let table = Table::new("users", datasource)
            .with_id_column("id")
            .with_column("name");

        assert_eq!(table.id_field().map(|c| c.name()), Some("id"));
    }

    #[test]
    fn test_table_auto_set_title_field() {
        let datasource = MockTableSource::new();
        let table = Table::new("users", datasource)
            .with_id_column("id")
            .with_title_column("name");

        assert_eq!(table.title_field().map(|c| c.name()), Some("name"));
    }

    #[test]
    fn test_table_first_wins_for_id_field() {
        let datasource = MockTableSource::new();
        let table = Table::new("users", datasource)
            .with_id_column("id")
            .with_id_column("alt_id");

        assert_eq!(table.id_field().map(|c| c.name()), Some("id"));
    }

    #[test]
    fn test_table_first_wins_for_title_field() {
        let datasource = MockTableSource::new();
        let table = Table::new("users", datasource)
            .with_title_column("name")
            .with_title_column("title");

        assert_eq!(table.title_field().map(|c| c.name()), Some("name"));
    }

    #[test]
    fn test_column_ordering() {
        use crate::with_ordering::SortDirection;

        let datasource = MockTableSource::new();
        let table = Table::new("users", datasource)
            .with_column("name")
            .with_column("age");

        let mut table = table;

        // Test column.ascending()
        table.add_order(table["name"].ascending());
        assert_eq!(table.orders().count(), 1);

        // Test column.descending()
        table.add_order(table["age"].descending());
        assert_eq!(table.orders().count(), 2);

        // Verify directions
        let orders: Vec<_> = table.orders().collect();
        assert!(matches!(orders[0].1, SortDirection::Ascending));
        assert!(matches!(orders[1].1, SortDirection::Descending));
    }
}
