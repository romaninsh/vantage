//! The tap over a two-pass Dio: the list page's correlated dispatch and
//! return, and the detail pass over the first viewport.

use vantage_vista::Vista;
use vantage_vista::mocks::MockShell;

use crate::support;
use crate::support::capture::{capture, lines_containing};
use crate::{tap_lens, wait_until};

/// Two-pass (list/detail) lifecycle: a list pass over a 40-row index (one
/// page — the default page size of 100 covers it in a single dispatch) then a
/// detail pass over the first viewport. Asserts the list page's dispatch/
/// return are correlated by `req`, and that a "detail pass" line reports the
/// 10-row viewport as `requested=10`.
#[tokio::test]
async fn two_pass_list_and_detail_lifecycle_is_logged() {
    let (_guard, log) = capture();

    let ids: Vec<String> = (0..40).map(|i| format!("id{i}")).collect();
    let listed = ids.iter().enumerate().fold(
        MockShell::new().with_metadata(support::meta(&["id", "name"])),
        |shell, (i, id)| {
            shell.with_record(
                id,
                support::record(&[("id", id), ("name", &format!("Row {i}"))]),
            )
        },
    );
    let details = ids.iter().fold(
        MockShell::new().with_metadata(support::meta(&["id", "detail"])),
        |shell, id| {
            let detail = format!("Detail for {id}");
            shell.with_record(id, support::record(&[("id", id), ("detail", &detail)]))
        },
    );
    let dio = tap_lens(true, None)
        .dio(Vista::new("books", listed))
        .augment(vec![support::fixed_augmentation(
            Vista::new("details", details.get_only()),
            &["detail"],
        )])
        .build()
        .await
        .unwrap();
    let scenery = dio.table_scenery().open().await.unwrap();

    // The seed list page runs detached from `open()`; wait for it to land.
    wait_until("list page return", || {
        lines_containing(&log, "list #1 got").len() == 1
    })
    .await;

    let dispatch = lines_containing(&log, "list #1 asks");
    let ret = lines_containing(&log, "list #1 got");
    assert_eq!(dispatch.len(), 1, "{dispatch:?}");
    assert_eq!(ret.len(), 1, "{ret:?}");
    let req = dispatch[0]
        .split("list #")
        .nth(1)
        .expect("list dispatch names its request")
        .split_whitespace()
        .next()
        .expect("request id")
        .to_string();
    assert!(
        ret[0].contains(&format!("list #{req} got")),
        "the return must carry the same request id: {}",
        ret[0]
    );
    assert!(dispatch[0].contains("offset 0"), "{}", dispatch[0]);
    assert!(dispatch[0].contains("ids from"), "{}", dispatch[0]);
    assert!(ret[0].contains("40 ids"), "{}", ret[0]);
    assert!(ret[0].contains("40 known so far"), "{}", ret[0]);
    assert!(ret[0].contains("all of them"), "{}", ret[0]);

    // Detail pass over the first 10 rows.
    scenery.set_viewport(0..10, 0..0);
    wait_until("detail pass requested=10", || {
        lines_containing(&log, "hydrate")
            .iter()
            .any(|l| l.contains("10 rows in view"))
    })
    .await;

    let detail_lines = lines_containing(&log, "hydrate");
    assert!(
        detail_lines.iter().any(|l| l.contains("10 rows in view")),
        "{detail_lines:?}"
    );
    assert!(
        detail_lines[0].contains("need detail"),
        "{}",
        detail_lines[0]
    );
}
