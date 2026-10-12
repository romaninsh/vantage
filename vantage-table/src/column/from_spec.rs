use vantage_core::Result;
use vantage_vista::ColumnSpec;

use super::core::{Column, ColumnType};
use super::flags::ColumnFlag;

impl<A: ColumnType> Column<A> {
    /// Lower a YAML spec column. Its `type:` (default `string`) goes through
    /// the driver's `for_type` map; then the driver's physical name `alias`
    /// applies (unless it repeats `name`), and every spec flag with a typed
    /// counterpart carries over. `id` and `title` don't: the table records
    /// those designations itself (`set_spec_id_field`, `add_spec_column`).
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
        let carried = [
            ColumnFlag::Mandatory,
            ColumnFlag::Hidden,
            ColumnFlag::Searchable,
            ColumnFlag::Indexed,
            ColumnFlag::Label,
            ColumnFlag::RequestParam,
        ];
        for flag in carried {
            if spec.has_flag(flag.to_vista()) {
                column = column.with_flag(flag);
            }
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
        let spec = ColumnSpec::<NoExtras>::new().with_flag(vantage_vista::ColumnFlag::Hidden);
        let column =
            Column::from_spec("name", &spec, Some("db_name".into()), string_column).unwrap();
        assert_eq!(column.alias(), Some("db_name"));
        assert!(column.flags().contains(&ColumnFlag::Hidden));
    }

    #[test]
    fn request_param_carries_over() {
        let spec = ColumnSpec::<NoExtras>::new().with_flag("request_param");
        let column = Column::from_spec("region", &spec, None, string_column).unwrap();
        assert!(column.flags().contains(&ColumnFlag::RequestParam));
        assert!(!column.flags().contains(&ColumnFlag::Hidden));
    }

    #[test]
    fn presentation_and_search_flags_carry_over() {
        let spec = ColumnSpec::<NoExtras>::new()
            .with_flag("label")
            .with_flag("searchable")
            .with_flag("mandatory")
            .with_flag("indexed");
        let column = Column::from_spec("status", &spec, None, string_column).unwrap();
        for flag in [
            ColumnFlag::Label,
            ColumnFlag::Searchable,
            ColumnFlag::Mandatory,
            ColumnFlag::Indexed,
        ] {
            assert!(column.flags().contains(&flag), "{flag:?}");
        }
    }

    #[test]
    fn alias_equal_to_name_is_dropped() {
        let spec = ColumnSpec::<NoExtras>::new();
        let column = Column::from_spec("name", &spec, Some("name".into()), string_column).unwrap();
        assert_eq!(column.alias(), None);
    }
}
