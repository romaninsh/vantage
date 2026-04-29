use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::{AwsAccount, eq};

use super::service::{Service, services_table};
use super::task::{Task, tasks_table};

/// One ECS cluster — `ListClusters` returns just an ARN per row, so
/// that's all you get without a follow-up `DescribeClusters` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cluster {
    #[serde(rename = "clusterArn")]
    pub cluster_arn: String,
}

/// `ListClusters` table — every cluster in the account / region.
///
/// ```no_run
/// # use vantage_aws::AwsAccount;
/// # use vantage_aws::models::ecs::clusters_table;
/// # async fn run() -> vantage_core::Result<()> {
/// # let aws = AwsAccount::from_default()?;
/// let clusters = clusters_table(aws);
/// # Ok(()) }
/// ```
pub fn clusters_table(aws: AwsAccount) -> Table<AwsAccount, Cluster> {
    Table::new(
        "json1/clusterArns:ecs/AmazonEC2ContainerServiceV20141113.ListClusters",
        aws,
    )
    .with_id_column("clusterArn")
}

impl Cluster {
    /// The cluster's short name, parsed out of [`Self::cluster_arn`].
    /// Cluster ARNs have the shape
    /// `arn:aws:ecs:<region>:<account>:cluster/<name>`.
    pub fn name(&self) -> Option<&str> {
        self.cluster_arn.rsplit('/').next()
    }

    /// Services table pre-filtered to *this* cluster.
    pub fn ref_services(&self, aws: AwsAccount) -> Table<AwsAccount, Service> {
        let mut t = services_table(aws);
        t.add_condition(eq("cluster", self.cluster_arn.clone()));
        t
    }

    /// Tasks table pre-filtered to *this* cluster.
    pub fn ref_tasks(&self, aws: AwsAccount) -> Table<AwsAccount, Task> {
        let mut t = tasks_table(aws);
        t.add_condition(eq("cluster", self.cluster_arn.clone()));
        t
    }
}
