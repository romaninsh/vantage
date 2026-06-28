use vantage_expressions::traits::datasource::DataSource;

use crate::cluster::KubernetesCluster;

impl DataSource for KubernetesCluster {}
