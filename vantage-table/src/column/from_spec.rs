use vantage_core::Result;
use vantage_vista::{ColumnSpec, flags};

use super::core::{Column, ColumnType};
use super::flags::ColumnFlag;

impl<A: ColumnType> Column<A> {
    /// Lower a YAML spec column. Its `type:` (default `string`) goes through
    /// the driver's `for_type` map; then the driver's physical name `alias`
    /// applies (unless it repeats `name`), and the `hidden` flag carries over.
    pub fn from_spec<C>(
        name: &str,
        spec: &ColumnSpec<C>,
        alias: Option<String>,
        for_type: impl FnOnce(&str, &str) -> Result<Self>,
    ) -> Result<Self> {
        let mut column = for_type(name, spec.col_type.as_deref().unwrap_or("string"))?;
        if let Some(alias) = alias.filter(|a| a != name) {
            column = column.with_alias(alias);
        }
        if spec.has_flag(flags::HIDDEN) {
            column = column.with_flag(ColumnFlag::Hidden);
        }
        Ok(column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_vista::NoExtras;

    fn string_column(name: &str, _ty: &str) -> Result<Column<String>> {
        Ok(Column::new(name))
    }

    #[test]
    fn alias_and_hidden_carry_over() {
        let spec = ColumnSpec::<NoExtras>::new().with_flag(flags::HIDDEN);
        let column =
            Column::from_spec("name", &spec, Some("db_name".into()), string_column).unwrap();
        assert_eq!(column.alias(), Some("db_name"));
        assert!(column.flags().contains(&ColumnFlag::Hidden));
    }

    #[test]
    fn alias_equal_to_name_is_dropped() {
        let spec = ColumnSpec::<NoExtras>::new();
        let column = Column::from_spec("name", &spec, Some("name".into()), string_column).unwrap();
        assert_eq!(column.alias(), None);
    }
}
