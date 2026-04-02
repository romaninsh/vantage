use super::config::VantageConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuItem {
    pub key: String,
    pub name: Option<String>,
    pub icon: Option<String>,
}

impl VantageConfig {
    pub fn menu_items(&self) -> Vec<MenuItem> {
        self.menu
            .as_ref()
            .map(|menu| {
                menu.iter()
                    .flat_map(|item_map| {
                        item_map.iter().map(|(key, config)| MenuItem {
                            key: key.clone(),
                            name: config.name.clone(),
                            icon: config.icon.clone(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
