use jsonschema::JSONSchema;
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use vantage_core::{error, util::error::Context, Result};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VantageConfig {
    /// Array of menu items for the navigation sidebar
    #[serde(skip_serializing_if = "Option::is_none")]
    pub menu: Option<Vec<HashMap<String, MenuItemConfig>>>,
    /// Role-based permissions configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<HashMap<String, RoleConfig>>,
    /// Entity definitions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<HashMap<String, EntityConfig>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MenuItemConfig {
    /// Display name for the menu item (optional, will use key if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Icon name for the menu item
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RoleConfig(Vec<PermissionRule>);

impl RoleConfig {
    pub fn rules(&self) -> &[PermissionRule] {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PermissionRule {
    pub allow: PermissionType,
    pub on: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PermissionType {
    All(String), // "*"
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EntityConfig {
    /// Database table name
    pub table: String,
    /// ID column name
    pub id_column: String,
    /// Column definitions
    pub columns: Vec<ColumnConfig>,
    /// Relationship definitions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relations: Option<Vec<RelationConfig>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RelationConfig {
    /// Relation type: "belongs_to" or "has_many"
    #[serde(rename = "type")]
    pub rel_type: String,
    /// Name of the relation (used in ref_xxx methods)
    pub name: String,
    /// Foreign key column name
    pub foreign_key: String,
    /// Target entity name
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ColumnConfig {
    /// Column name
    pub name: String,
    /// Column type (defaults to "any" if not specified)
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub col_type: Option<String>,
    /// Column flags (mandatory, hidden, id, title, searchable)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    /// Default value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    /// Validation rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<HashMap<String, Value>>,
}

impl VantageConfig {
    /// Load and validate config from YAML file
    pub fn from_file<P: AsRef<Path>>(file_path: P) -> Result<Self> {
        let schema = schema_for!(VantageConfig);
        let compiled_schema = JSONSchema::compile(
            &serde_json::to_value(schema).context("Failed to serialize schema")?,
        )
        .map_err(|e| error!("Failed to compile schema", error = e.to_string()))?;

        Self::load_and_validate(&file_path, &compiled_schema)
    }

    /// Generate and return the JSON Schema as a pretty-printed string
    pub fn get_schema_json() -> Result<String> {
        let schema = schema_for!(VantageConfig);
        serde_json::to_string_pretty(&schema).context("Failed to serialize schema to JSON")
    }

    /// Write the JSON Schema to a file
    pub fn write_schema_file<P: AsRef<Path>>(schema_path: P) -> Result<()> {
        let schema_json = Self::get_schema_json()?;
        std::fs::write(schema_path, schema_json).context("Failed to write schema file")?;
        Ok(())
    }

    fn load_and_validate<P: AsRef<Path>>(file_path: P, schema: &JSONSchema) -> Result<Self> {
        // Load and parse YAML
        let content = std::fs::read_to_string(&file_path).with_context(|| {
            error!(
                "Failed to read config file",
                path = file_path.as_ref().display().to_string()
            )
        })?;
        let yaml_value: serde_yaml::Value = serde_yaml::from_str(&content)
            .map_err(|e| error!("YAML parsing error", error = e.to_string()))?;

        // Convert YAML to JSON for schema validation
        let json_value: Value = serde_json::to_value(&yaml_value)
            .map_err(|e| error!("YAML to JSON conversion error", error = e.to_string()))?;

        // Validate against schema
        if let Err(errors) = schema.validate(&json_value) {
            let error_messages: Vec<String> = errors
                .map(|error| {
                    format!(
                        "Schema validation error at {}: {}",
                        error.instance_path, error
                    )
                })
                .collect();
            return Err(error!(
                "Schema validation failed",
                errors = error_messages.join("\n")
            ));
        }

        // Parse into our config struct
        let config: VantageConfig = serde_json::from_value(json_value)
            .map_err(|e| error!("Config deserialization error", error = e.to_string()))?;

        // Additional business logic validation
        Self::validate_business_rules(&config)
            .map_err(|e| error!("Business rules validation failed", error = e))?;

        Ok(config)
    }

    fn validate_business_rules(config: &VantageConfig) -> std::result::Result<(), String> {
        // Check for duplicate menu keys
        if let Some(menu) = &config.menu {
            let mut seen_keys = std::collections::HashSet::new();
            for menu_item in menu {
                for key in menu_item.keys() {
                    if !seen_keys.insert(key) {
                        return Err(format!("Duplicate menu key '{}'", key));
                    }
                }
            }

            // Validate icon names
            let valid_icons = ["User", "Bot", "Map", "Star", "Settings", "Inbox"];
            for menu_item in menu {
                for (key, item_config) in menu_item {
                    if let Some(icon) = &item_config.icon {
                        if !valid_icons.contains(&icon.as_str()) {
                            return Err(format!(
                                "Menu item '{}' has invalid icon '{}'. Valid icons: {}",
                                key,
                                icon,
                                valid_icons.join(", ")
                            ));
                        }
                    }
                }
            }
        }

        // Validate entities
        if let Some(entities) = &config.entities {
            for (entity_name, entity) in entities {
                // Check for duplicate column names
                let mut seen_columns = std::collections::HashSet::new();
                for column in &entity.columns {
                    if !seen_columns.insert(&column.name) {
                        return Err(format!(
                            "Entity '{}' has duplicate column '{}'",
                            entity_name, column.name
                        ));
                    }
                }
            }
        }

        Ok(())
    }
}
