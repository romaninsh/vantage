use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::AwsAccount;

/// One ECS task definition revision — `ListTaskDefinitions` returns
/// just an ARN per row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDefinition {
    #[serde(rename = "taskDefinitionArn")]
    pub task_definition_arn: String,
}

/// `ListTaskDefinitions` table — every active task definition in the
/// account. Optional filters: `familyPrefix`, `status` (`ACTIVE` /
/// `INACTIVE`), `sort` (`ASC` / `DESC`).
pub fn task_definitions_table(aws: AwsAccount) -> Table<AwsAccount, TaskDefinition> {
    Table::new(
        "json1/taskDefinitionArns:ecs/AmazonEC2ContainerServiceV20141113.ListTaskDefinitions",
        aws,
    )
    .with_id_column("taskDefinitionArn")
}

impl TaskDefinition {
    /// The `family:revision` part of the ARN — the form most ECS APIs
    /// will accept as input.
    pub fn family_revision(&self) -> Option<&str> {
        self.task_definition_arn.rsplit('/').next()
    }
}
