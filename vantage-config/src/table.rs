use crate::config::{EntityConfig, VantageConfig};
use vantage_surrealdb::{SurrealColumn, SurrealDB};
use vantage_table::{ColumnFlag, EmptyEntity, Table};

impl VantageConfig {
    /// Get a table builder for a named entity
    pub fn get_table(
        &self,
        entity_name: &str,
        db: SurrealDB,
    ) -> Option<Table<SurrealDB, EmptyEntity>> {
        let entities = self.entities.as_ref()?;
        let entity = entities.get(entity_name)?;

        Some(Self::build_table(entity, db))
    }

    fn build_table(entity: &EntityConfig, db: SurrealDB) -> Table<SurrealDB, EmptyEntity> {
        let mut table = Table::new(&entity.table, db);

        for column in &entity.columns {
            let col_type = column.col_type.as_deref().unwrap_or("any");

            // Build column based on type
            let mut flags = Vec::new();
            if !column.optional {
                flags.push(ColumnFlag::Mandatory);
            }

            // Create typed column based on config
            let col = match col_type {
                "string" => {
                    let mut col = SurrealColumn::<String>::new(&column.name);
                    if !flags.is_empty() {
                        col = col.with_flags(&flags);
                    }
                    col.into_any()
                }
                "int" => {
                    let mut col = SurrealColumn::<i64>::new(&column.name);
                    if !flags.is_empty() {
                        col = col.with_flags(&flags);
                    }
                    col.into_any()
                }
                "float" => {
                    let mut col = SurrealColumn::<f64>::new(&column.name);
                    if !flags.is_empty() {
                        col = col.with_flags(&flags);
                    }
                    col.into_any()
                }
                "bool" => {
                    let mut col = SurrealColumn::<bool>::new(&column.name);
                    if !flags.is_empty() {
                        col = col.with_flags(&flags);
                    }
                    col.into_any()
                }
                "datetime" => {
                    // Use SurrealDB's datetime type wrapper
                    let mut col =
                        SurrealColumn::<surreal_client::types::DateTime>::new(&column.name);
                    if !flags.is_empty() {
                        col = col.with_flags(&flags);
                    }
                    col.into_any()
                }
                "decimal" => {
                    // Decimal type - requires decimal feature
                    #[cfg(feature = "decimal")]
                    {
                        let mut col =
                            SurrealColumn::<surreal_client::types::Decimal>::new(&column.name);
                        if !flags.is_empty() {
                            col = col.with_flags(&flags);
                        }
                        col.into_any()
                    }
                    #[cfg(not(feature = "decimal"))]
                    {
                        // Fallback to string if decimal feature not enabled
                        let mut col = SurrealColumn::<String>::new(&column.name);
                        if !flags.is_empty() {
                            col = col.with_flags(&flags);
                        }
                        col.into_any()
                    }
                }
                "duration" => {
                    let mut col =
                        SurrealColumn::<surreal_client::types::Duration>::new(&column.name);
                    if !flags.is_empty() {
                        col = col.with_flags(&flags);
                    }
                    col.into_any()
                }
                _ => {
                    // Default to untyped column
                    SurrealColumn::<surreal_client::types::Any>::new(&column.name).into_any()
                }
            };

            table = table.with_column(col);
        }

        table.into_entity()
    }
}
