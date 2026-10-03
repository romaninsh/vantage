use crate::rhai::bridge::block_on;

#[test]
fn ready_future_runs_without_runtime() {
    assert_eq!(block_on(async { 7 }).unwrap(), 7);
}

#[test]
fn pending_future_without_runtime_errors() {
    let err = block_on(std::future::pending::<()>()).unwrap_err();
    assert!(err.to_string().contains("needs a tokio runtime"));
}

#[tokio::test(flavor = "multi_thread")]
async fn runtime_future_runs_under_spawn_blocking() {
    let out = tokio::task::spawn_blocking(|| {
        block_on(async {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            3
        })
    })
    .await
    .unwrap();
    assert_eq!(out.unwrap(), 3);
}
