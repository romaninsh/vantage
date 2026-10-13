//! Shell behaviour that needs no cluster: schema, narrowing refusals and
//! relation targets. The client points at an unroutable address and no test
//! here issues a request.

use vantage_kubernetes::KubernetesCluster;
use vantage_kubernetes::models::Factory;
use vantage_kubernetes::models::core::pods;
use vantage_vista::{FilterOp, Vista};

fn offline_cluster() -> KubernetesCluster {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = kube::Config::new("http://127.0.0.1:9".parse().unwrap());
    KubernetesCluster::new(kube::Client::try_from(config).unwrap())
}

fn pods_vista(cluster: &KubernetesCluster) -> Vista {
    cluster
        .vista_factory()
        .from_table(pods::pods_table(cluster.clone()))
        .unwrap()
}

#[tokio::test]
async fn conditions_need_a_declared_column_and_equality() {
    let cluster = offline_cluster();
    let mut vista = pods_vista(&cluster);

    let op = vista
        .add_condition("restarts", FilterOp::Gt, 1_i64)
        .unwrap_err();
    assert!(op.is_unsupported(), "{op:?}");
    let unknown = vista
        .add_condition("colour", FilterOp::Eq, "red")
        .unwrap_err();
    assert!(!unknown.is_unsupported(), "{unknown:?}");
    // A path into a declared column is refused, for the caller to evaluate.
    let path = vista
        .add_condition("namespace.part", FilterOp::Eq, "demo")
        .unwrap_err();
    assert!(path.is_unimplemented(), "{path:?}");
    assert!(vista.query().is_empty());

    let narrowed = vista
        .clone()
        .with_condition("namespace", FilterOp::Eq, "demo")
        .unwrap()
        .with_search("web")
        .unwrap();
    assert!(narrowed.is_narrowed());
    assert!(
        !vista.is_narrowed(),
        "narrowing a clone leaves the original"
    );
    let note = narrowed.preview().note.unwrap();
    assert!(
        note.contains("namespace") && note.contains("\"web\""),
        "{note}"
    );
}

#[tokio::test]
async fn an_unknown_order_column_is_an_error() {
    let cluster = offline_cluster();
    let err = pods_vista(&cluster)
        .with_order("colour", vantage_vista::SortDirection::Ascending)
        .unwrap_err();
    assert!(!err.is_unsupported(), "{err:?}");
}

/// Every relation names its target by the target's table name. Joins are
/// strict, so the target must declare the column its join key constrains.
/// Building a target issues no request.
#[tokio::test]
async fn relation_targets_are_named_and_declare_their_join_columns() {
    let factory = Factory::new(offline_cluster());
    for name in Factory::known_names() {
        let (vista, _) = factory
            .for_name(name)
            .unwrap_or_else(|| panic!("{name} must build"));
        for reference in vista.references().values() {
            let target = vista.get_ref(&reference.name, None).unwrap();
            assert_eq!(reference.target, target.name(), "{name}:{}", reference.name);
            for key in &reference.keys {
                let column = key.target_column(&target).unwrap();
                assert!(
                    target.column(column).is_some(),
                    "{name}:{} target `{}` lacks join column `{column}`",
                    reference.name,
                    target.name()
                );
            }
        }
    }
}
