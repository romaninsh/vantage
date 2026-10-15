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
