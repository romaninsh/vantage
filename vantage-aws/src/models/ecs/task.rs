use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::{AwsAccount, eq};

use crate::models::logs::event::{LogEvent, events_table};

/// One ECS task — `ListTasks` returns just an ARN per row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    #[serde(rename = "taskArn")]
    pub task_arn: String,
}

/// `ListTasks` table. AWS requires `cluster` (a name or ARN) before it
/// will list anything, so add `eq("cluster", "...")` first or traverse
/// from a [`Cluster`](super::cluster::Cluster).
///
/// Also accepts the optional filters AWS supports as conditions:
/// `serviceName`, `family`, `desiredStatus` (`RUNNING` / `PENDING` /
/// `STOPPED`), `launchType`, `containerInstance`.
pub fn tasks_table(aws: AwsAccount) -> Table<AwsAccount, Task> {
    Table::new(
        "json1/taskArns:ecs/AmazonEC2ContainerServiceV20141113.ListTasks",
        aws,
    )
    .with_id_column("taskArn")
}

impl Task {
    /// The task's id, parsed out of [`Self::task_arn`]. ECS task ARNs
    /// have the shape `arn:aws:ecs:<region>:<account>:task/<cluster>/<id>`.
    pub fn task_id(&self) -> Option<&str> {
        self.task_arn.rsplit('/').next()
    }

    /// Log events from the given log group, with `logStreamNamePrefix`
    /// set to this task's id.
    ///
    /// ECS log streams typically follow the pattern
    /// `<streamPrefix>/<containerName>/<taskId>` — task id is at the
    /// END, so this prefix-match doesn't directly find a task's
    /// streams. Use it instead to point at a known prefix and combine
    /// with a `filterPattern` that includes the task id.
    ///
    /// Most callers will already know the streamPrefix from the task
    /// definition; pass that as `prefix` instead of the raw task id.
    pub fn ref_log_events(
        &self,
        aws: AwsAccount,
        log_group_name: &str,
        prefix: &str,
    ) -> Table<AwsAccount, LogEvent> {
        let mut t = events_table(aws);
        t.add_condition(eq("logGroupName", log_group_name));
        t.add_condition(eq("logStreamNamePrefix", prefix));
        t
    }
}
