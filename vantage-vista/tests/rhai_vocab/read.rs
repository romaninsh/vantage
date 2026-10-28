use std::sync::{Arc, Mutex};

use serde_json::json;
use vantage_core::Result;
use vantage_vista::mocks::MockShell;
use vantage_vista::rhai::{Terminals, Writes};
use vantage_vista::{
    Page, TableShell, Vista, VistaCapabilities, VistaResolver, forward_table_shell,
};

use super::support::{host, host_with, json, read_write, read_write_terminals, rec, run, store};

fn capped(limit: usize) -> vantage_rhai::Host {
    host(
        &store(),
        Terminals::ReadWrite {
            limit: Some(limit),
            writes: Writes::Allowed,
        },
    )
}

#[test]
fn a_host_limit_caps_every_list_and_a_smaller_script_limit_applies() {
    let host = capped(2);
    let rows = json(&host, r#"table("t").list()"#);
    assert_eq!(rows.as_array().unwrap().len(), 2);
    let rows = json(&host, r#"table("t").limit(1).list()"#);
    assert_eq!(rows.as_array().unwrap().len(), 1);
    let rows = json(&host, r#"table("t").limit(9).list()"#);
    assert_eq!(
        rows.as_array().unwrap().len(),
        2,
        "the host cap still holds"
    );
    let rows = json(&host, r#"table("t").offset(2).list()"#);
    assert_eq!(
        rows,
        json!([{"id": "r3", "a": 1, "n": 2}]),
        "the cap counts after the offset"
    );
}

#[test]
fn an_uncapped_list_returns_every_row_with_its_id() {
    let rows = json(&read_write(&store()), r#"table("t").list()"#);
    assert_eq!(rows.as_array().unwrap().len(), 3);
    assert_eq!(rows[0]["id"], json!("r1"));
}

#[test]
fn get_missing_is_unit() {
    let host = read_write(&store());
    assert!(run(&host, r#"table("t").get("nope")"#).unwrap().is_unit());
    assert_eq!(json(&host, r#"table("t").get("r2")"#)["a"], json!(2));
}

#[test]
fn first_follows_sort() {
    let row = json(&read_write(&store()), r#"table("t").sort("n").first()"#);
    assert_eq!(row["id"], json!("r2"));
}

/// The `(offset, limit)` of every window the shell was asked for.
type Windows = Arc<Mutex<Vec<(usize, usize)>>>;

/// Records every window the shell is asked for.
#[derive(Clone)]
struct WindowSpy {
    inner: Box<dyn TableShell>,
    windows: Windows,
}

forward_table_shell!(WindowSpy, inner, {
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        self.windows.lock().unwrap().push((offset, limit));
        self.inner.fetch_window(offset, limit).await
    }
});

/// Table `m` over a mock holding r1..r5, uncapped; `windowed` keeps the
/// mock's `Window`, otherwise it advertises nothing. Returns the windows
/// asked for too.
fn mock_host(windowed: bool) -> (vantage_rhai::Host, Windows) {
    let mut mock = MockShell::new();
    if !windowed {
        mock = mock.with_capabilities(VistaCapabilities::new());
    }
    for i in 1..=5 {
        mock = mock.with_record(format!("r{i}"), rec(&[("i", i)]));
    }
    let windows = Arc::new(Mutex::new(Vec::new()));
    let spy = WindowSpy {
        inner: Box::new(mock),
        windows: windows.clone(),
    };
    let resolver: Arc<dyn VistaResolver> =
        Arc::new(move |name: &str| Ok(Vista::new(name, spy.clone())));
    (host_with(resolver, read_write_terminals()), windows)
}

fn take_windows(windows: &Mutex<Vec<(usize, usize)>>) -> Vec<(usize, usize)> {
    std::mem::take(&mut *windows.lock().unwrap())
}

#[test]
fn offset_reads_the_window_at_its_position_on_a_windowed_shell() {
    let (host, windows) = mock_host(true);
    for script in [
        r#"table("m").offset(1).limit(2).ids()"#,
        r#"table("m").limit(2).offset(1).ids()"#,
    ] {
        assert_eq!(json(&host, script), json!(["r2", "r3"]), "{script}");
        assert_eq!(take_windows(&windows), [(1, 2)], "{script}");
    }
    // Offsets add up; limits keep the smallest.
    assert_eq!(
        json(
            &host,
            r#"table("m").offset(1).limit(3).offset(2).limit(9).ids()"#
        ),
        json!(["r4", "r5"])
    );
    assert_eq!(take_windows(&windows), [(3, 3)]);
    assert_eq!(
        json(&host, r#"table("m").offset(3).first()"#)["id"],
        json!("r4")
    );
    assert_eq!(take_windows(&windows), [(3, 1)]);
    // No limit and no host cap: listed, then skipped.
    assert_eq!(
        json(&host, r#"table("m").offset(2).ids()"#),
        json!(["r3", "r4", "r5"])
    );
    assert!(take_windows(&windows).is_empty());
}

#[test]
fn offset_over_a_shell_without_window_skips_the_rows_of_a_leading_window() {
    let (host, windows) = mock_host(false);
    assert_eq!(
        json(&host, r#"table("m").offset(1).limit(2).ids()"#),
        json!(["r2", "r3"])
    );
    assert_eq!(take_windows(&windows), [(0, 3)]);
    assert_eq!(
        json(&host, r#"table("m").offset(3).first()"#)["id"],
        json!("r4")
    );
    assert_eq!(take_windows(&windows), [(0, 4)]);
    assert_eq!(
        json(&host, r#"table("m").offset(2).ids()"#),
        json!(["r3", "r4", "r5"])
    );
}

#[test]
fn count_with_offset_counts_the_rows_list_returns() {
    let (host, _) = mock_host(true);
    for (script, count) in [
        (r#"table("m").offset(1).count()"#, 4),
        (r#"table("m").offset(1).limit(2).count()"#, 2),
        (r#"table("m").offset(4).limit(3).count()"#, 1),
        (r#"table("m").offset(5).count()"#, 0),
        (r#"table("m").offset(0).count()"#, 5),
    ] {
        assert_eq!(json(&host, script), json!(count), "{script}");
    }
}

#[test]
fn an_offset_past_the_end_reads_nothing() {
    for windowed in [true, false] {
        let (host, _) = mock_host(windowed);
        assert_eq!(json(&host, r#"table("m").offset(9).list()"#), json!([]));
        assert_eq!(
            json(&host, r#"table("m").offset(9).limit(2).ids()"#),
            json!([])
        );
        assert!(
            run(&host, r#"table("m").offset(9).first()"#)
                .unwrap()
                .is_unit()
        );
    }
}

#[test]
fn a_negative_offset_is_refused() {
    let (host, _) = mock_host(true);
    let err = run(&host, r#"table("m").offset(-1)"#).unwrap_err();
    assert!(err.contains("offset can't be negative"), "{err}");
}

/// The fixture holds no text cells, so any search matches nothing.
#[test]
fn search_narrows() {
    let host = read_write(&store());
    assert_eq!(json(&host, r#"table("t").search("zz").count()"#), json!(0));
}
