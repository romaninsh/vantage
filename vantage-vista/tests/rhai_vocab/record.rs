//! `record()`/`record(id)` on the `Table` handle: staged edits, `flash()` as
//! an insert or a patch of only the changed fields, delete, revert, and the
//! read-only id column.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use serde_json::json;
use vantage_core::Result;
use vantage_rhai::Host;
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Capability, Column, ColumnFlag, TableShell, Vista, VistaCapabilities, VistaMetadata,
    VistaResolver, forward_table_shell,
};

use super::support::{eval, host_with, json, read_write, read_write_terminals, run, store};

#[test]
fn new_record_flash_inserts_and_returns_id() {
    let host = read_write(&store());
    let id: String = eval(
        &host,
        r#"
        let r = table("t").record();
        r.a = 9;
        r.flash()
    "#,
    );
    assert_eq!(
        json(&host, &format!(r#"table("t").get("{id}")"#))["a"],
        json!(9)
    );
}

#[test]
fn retried_new_record_flash_reuses_its_id() {
    let caps = VistaCapabilities::new().with(Capability::Insert);
    let shell = SpyShell::new(mock_with("r1", &[], caps));
    shell.lose_next_insert.store(true, Ordering::SeqCst);
    let host = host_over(shell);
    let out = json(
        &host,
        r#"
        let r = table("t").record();
        r.a = 5;
        let first = "flashed";
        try {
            r.flash();
        } catch(err) {
            first = r.status();
        }
        let id = r.flash();
        #{ first: first, id: id, ids: table("t").ids() }
    "#,
    );
    assert_eq!(out["first"], json!("failed"), "{out}");
    assert_eq!(out["ids"], json!(["r1", out["id"]]), "{out}");
}

#[test]
fn unchanged_flash_writes_nothing() {
    // Update is not advertised; if flash() attempted a patch despite no
    // staged changes, the Vista would refuse it and the script would throw.
    let host = host_over(mock_with(
        "r1",
        &[("n", CborValue::Integer(3.into()))],
        VistaCapabilities::new(),
    ));
    let id: String = eval(
        &host,
        r#"
        let r = table("t").record("r1");
        r.flash()
    "#,
    );
    assert_eq!(id, "r1");
}

#[test]
fn loaded_record_flashes_only_changed_fields() {
    let shell = SpyShell::new(mock_with(
        "r1",
        &[
            ("a", CborValue::Integer(1.into())),
            ("b", CborValue::Integer(2.into())),
        ],
        VistaCapabilities::new().with(Capability::Update),
    ));
    let patches = shell.patches.clone();
    let host = host_over(shell);
    let _ = run(
        &host,
        r#"
        let r = table("t").record("r1");
        r.a = 99;
        r.flash();
    "#,
    )
    .unwrap();

    let logged = patches.lock().unwrap();
    assert_eq!(logged.len(), 1);
    let keys: Vec<&str> = logged[0].as_inner().keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["a"]);
}

#[test]
fn dotted_set_builds_nested_map() {
    let host = read_write(&store());
    let out = json(
        &host,
        r#"
        let r = table("t").record();
        r.set(#{ "inventory.stock": 12, name: "Widget" });
        let id = r.flash();
        table("t").get(id)
    "#,
    );
    assert_eq!(out["inventory"]["stock"], json!(12));
    assert_eq!(out["name"], json!("Widget"));
}

#[test]
fn sibling_dotted_keys_merge_into_one_nested_map_and_replace_a_scalar() {
    let host = read_write(&store());
    let out = json(
        &host,
        r#"
        let r = table("t").record();
        r.set(#{ "meta.a": 1 });
        r.set(#{ "meta.b": 2 });
        r.set(#{ "meta.a": 3 });
        r.set(#{ "deep.x.y": 4 });
        r.set(#{ flat: 1 });
        r.set(#{ "flat.inner": 5 });
        table("t").get(r.flash())
    "#,
    );
    assert_eq!(out["meta"], json!({"a": 3, "b": 2}));
    assert_eq!(out["deep"], json!({"x": {"y": 4}}));
    assert_eq!(out["flat"], json!({"inner": 5}));
}

#[test]
fn a_record_on_a_missing_id_is_a_not_found_error() {
    let host = read_write(&store());
    let err = run(&host, r#"table("t").record("missing")"#).unwrap_err();
    assert!(err.contains("not found"), "{err}");
}

#[test]
fn the_id_of_a_record_is_read_only() {
    let host = read_write(&store());
    let err = run(
        &host,
        r#"
        let r = table("t").record("r1");
        r.id = "nope";
    "#,
    )
    .unwrap_err();
    assert!(err.contains("read-only"), "{err}");
}

#[test]
fn field_sets_resolve_the_id_column_once() {
    let shell = mock_with("r1", &[], VistaCapabilities::new());
    let calls = Arc::new(Mutex::new(0usize));
    let seen = calls.clone();
    let resolver: Arc<dyn VistaResolver> = Arc::new(move |name: &str| {
        *seen.lock().unwrap() += 1;
        Ok(Vista::new(name, shell.clone()))
    });
    let host = host_with(resolver, read_write_terminals());
    let _ = run(&host, r#"let r = table("t").record("r1");"#).unwrap();
    let after_load = *calls.lock().unwrap();
    let _ = run(
        &host,
        r#"
        let r = table("t").record("r1");
        r.a = 1;
        r.b = 2;
        r.set(#{ c: 3 });
    "#,
    )
    .unwrap();
    assert_eq!(
        *calls.lock().unwrap() - after_load,
        after_load + 1,
        "a load, then one id-column resolve for three sets"
    );
}

// ---- fixtures ---------------------------------------------------------

/// A `MockShell` seeded with one record under `id`, with `id_column("id")`
/// declared so `record()`'s read-only-id check has a column to compare
/// against.
fn mock_with(id: &str, fields: &[(&str, CborValue)], caps: VistaCapabilities) -> MockShell {
    let mut record: Record<CborValue> = Record::new();
    record.insert("id".to_string(), CborValue::Text(id.to_string()));
    for (k, v) in fields {
        record.insert(k.to_string(), v.clone());
    }
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_id_column("id");
    MockShell::new()
        .with_metadata(metadata)
        .with_capabilities(caps)
        .with_record(id, record)
}

/// A `ReadWrite`, unlimited host over a single fixed shell — every
/// `table(name)` call resolves to the same shell regardless of `name`.
fn host_over(shell: impl TableShell + Clone + 'static) -> Host {
    let resolver: Arc<dyn VistaResolver> =
        Arc::new(move |name: &str| Ok(Vista::new(name, shell.clone())));
    host_with(resolver, read_write_terminals())
}

/// Wraps a `MockShell` and records every `partial` a `patch` call carries,
/// so a test can assert on exactly what a record's `flash()` sent — not just
/// the final stored state, which a full-record patch would produce
/// identically (`MockShell::patch` merges either way). With
/// `lose_next_insert` set, the next insert lands but reports an error, the
/// way a write whose response is lost does.
#[derive(Clone)]
struct SpyShell {
    inner: Box<dyn TableShell>,
    patches: Arc<Mutex<Vec<Record<CborValue>>>>,
    lose_next_insert: Arc<AtomicBool>,
}

impl SpyShell {
    fn new(inner: MockShell) -> Self {
        Self {
            inner: Box::new(inner),
            patches: Arc::default(),
            lose_next_insert: Arc::default(),
        }
    }
}

forward_table_shell!(SpyShell, inner, {
    async fn patch(&self, id: &str, partial: &Record<CborValue>) -> Result<Record<CborValue>> {
        self.patches.lock().unwrap().push(partial.clone());
        self.inner.patch(id, partial).await
    }

    async fn insert(
        &self,
        id: Option<&str>,
        record: &Record<CborValue>,
    ) -> Result<(String, Record<CborValue>)> {
        let stored = self.inner.insert(id, record).await?;
        if self.lose_next_insert.swap(false, Ordering::SeqCst) {
            return Err(vantage_core::error!("connection reset"));
        }
        Ok(stored)
    }
});
