use crate::config::{EntityConfig, VantageConfig};
use vantage_surrealdb::{SurrealColumn, SurrealDB};
use vantage_table::{ColumnFlag, EmptyEntity, Table};

impl VantageConfig {
    /// Get a table builder for a named table
    pub fn get_table(
        &self,
        table_name: &str,
        db: SurrealDB,
    ) -> Option<Table<SurrealDB, EmptyEntity>> {
        let tables = self.tables.as_ref()?;
        let table_config = tables.get(table_name)?;

        Some(Self::build_table(table_config, db, self))
    }

    fn build_table(
        entity: &EntityConfig,
        db: SurrealDB,
        config: &VantageConfig,
    ) -> Table<SurrealDB, EmptyEntity> {
        let db_for_relations = db.clone();
        let mut table = Table::new(&entity.table, db);

        for column in &entity.columns {
            let col_type = column
                .col_type
                .as_ref()
                .map(|t| t.as_str())
                .unwrap_or("any");

            // Parse flags from config strings
            let mut flags = Vec::new();
            for flag_str in &column.flags {
                match flag_str.to_lowercase().as_str() {
                    "mandatory" => flags.push(ColumnFlag::Mandatory),
                    "hidden" => flags.push(ColumnFlag::Hidden),
                    "id" => flags.push(ColumnFlag::IdField),
                    "title" => flags.push(ColumnFlag::TitleField),
                    "searchable" => flags.push(ColumnFlag::Searchable),
                    _ => {} // Ignore unknown flags
                }
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

        // Add relationships if defined
        if let Some(relations) = &entity.relations {
            for relation in relations {
                let rel_type = relation.rel_type.as_str();
                let target = relation.target.clone();
                let foreign_key = relation.foreign_key.clone();
                let db_clone = db_for_relations.clone();
                let config_clone = config.clone();

                match rel_type {
                    "belongs_to" | "has_one" => {
                        table = table.with_one(&relation.name, &foreign_key, move || {
                            if let Some(tables) = &config_clone.tables {
                                if let Some(target_table) = tables.get(&target) {
                                    return Self::build_table(
                                        target_table,
                                        db_clone.clone(),
                                        &config_clone,
                                    );
                                }
                            }
                            // Fallback if target not found
                            Table::new(&target, db_clone.clone()).into_entity()
                        });
                    }
                    "has_many" => {
                        table = table.with_many(&relation.name, &foreign_key, move || {
                            if let Some(tables) = &config_clone.tables {
                                if let Some(target_table) = tables.get(&target) {
                                    return Self::build_table(
                                        target_table,
                                        db_clone.clone(),
                                        &config_clone,
                                    );
                                }
                            }
                            // Fallback if target not found
                            Table::new(&target, db_clone.clone()).into_entity()
                        });
                    }
                    _ => {
                        // Unknown relation type, skip
                    }
                }
            }
        }

        table.into_entity()
    }
}
