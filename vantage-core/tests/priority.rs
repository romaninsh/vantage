use vantage_core::Priority;

#[test]
fn default_is_background() {
    assert_eq!(Priority::default(), Priority::Background);
}

#[test]
fn only_essential_is_essential() {
    assert!(!Priority::Background.is_essential());
    assert!(Priority::Essential.is_essential());
}

#[test]
fn current_outside_a_runtime_is_background() {
    assert_eq!(Priority::current(), Priority::Background);
}

#[tokio::test]
async fn current_outside_any_scope_is_background() {
    assert_eq!(Priority::current(), Priority::Background);
}

#[tokio::test]
async fn scope_is_visible_to_nested_awaits_and_ends_with_the_future() {
    async fn deep() -> Priority {
        tokio::task::yield_now().await;
        Priority::current()
    }

    let seen = Priority::Essential.scope(async { deep().await }).await;
    assert_eq!(seen, Priority::Essential);
    assert_eq!(Priority::current(), Priority::Background);
}

#[tokio::test]
async fn scope_returns_the_future_output() {
    let out = Priority::Essential.scope(async { 7 }).await;
    assert_eq!(out, 7);
}

#[tokio::test]
async fn inner_scope_overrides_outer() {
    let seen = Priority::Essential
        .scope(async {
            Priority::Background
                .scope(async { Priority::current() })
                .await
        })
        .await;
    assert_eq!(seen, Priority::Background);
}

#[tokio::test]
async fn spawned_tasks_do_not_inherit_the_scope() {
    let seen = Priority::Essential
        .scope(async { tokio::spawn(async { Priority::current() }).await.unwrap() })
        .await;
    assert_eq!(seen, Priority::Background);
}

#[tokio::test]
async fn spawner_can_rewrap_the_spawned_future() {
    let seen = Priority::Essential
        .scope(async {
            tokio::spawn(Priority::Essential.scope(async { Priority::current() }))
                .await
                .unwrap()
        })
        .await;
    assert_eq!(seen, Priority::Essential);
}
