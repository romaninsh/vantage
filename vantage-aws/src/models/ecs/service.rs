use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::AwsAccount;

/// One ECS service — `ListServices` returns just an ARN per row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    #[serde(rename = "serviceArn")]
    pub service_arn: String,
}

/// `ListServices` table. AWS requires `cluster` (a name or ARN) before
/// it will list anything, so add `eq("cluster", "...")` first or
/// traverse from a [`Cluster`](super::cluster::Cluster).
pub fn services_table(aws: AwsAccount) -> Table<AwsAccount, Service> {
    Table::new(
        "serviceArns:ecs/AmazonEC2ContainerServiceV20141113.ListServices",
        aws,
    )
    .with_id_column("serviceArn")
}

impl Service {
    /// The service's short name, parsed out of [`Self::service_arn`].
    pub fn name(&self) -> Option<&str> {
        self.service_arn.rsplit('/').next()
    }
}
