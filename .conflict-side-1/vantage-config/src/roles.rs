use super::config::{PermissionType, RoleConfig, VantageConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub name: String,
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub action: Vec<String>,
    pub resource: String,
}

impl VantageConfig {
    pub fn roles(&self) -> Vec<Role> {
        self.roles
            .as_ref()
            .map(|roles_map| {
                roles_map
                    .iter()
                    .map(|(name, role_config)| Role {
                        name: name.clone(),
                        permissions: Self::role_config_to_permissions(role_config),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn role_config_to_permissions(role_config: &RoleConfig) -> Vec<Permission> {
        let rules = role_config.rules();
        rules
            .iter()
            .map(|rule| Permission {
                action: match &rule.allow {
                    PermissionType::All(s) if s == "*" => vec![
                        "list".to_string(),
                        "add".to_string(),
                        "edit".to_string(),
                        "delete".to_string(),
                        "view".to_string(),
                    ],
                    PermissionType::Single(action) => vec![action.clone()],
                    PermissionType::Multiple(actions) => actions.clone(),
                    PermissionType::All(action) => vec![action.clone()],
                },
                resource: rule.on.clone(),
            })
            .collect()
    }
}
