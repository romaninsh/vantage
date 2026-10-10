//! `record()`/`record(id)` on the `Table` handle: staged edits, save as an
//! insert or a patch of only the changed fields, delete, revert, and the
//! read-only id column.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::json;
use vantage_core::Result;
use vantage_rhai::{Host, Limits};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Column, DataVocab, Reference, TableShell, TargetResolver, Terminals, Vista, VistaCapabilities,
    VistaMetadata,
};

use super::support::{eval, host, json, read_write, run, store};

#[test]
fn new_record_save_inserts_and_returns_id() {
    let host = read_write(&store());
    let id: String = eval(
        &host,
        r#"
        let r = table("t").record();
        r.a = 9;
        r.save()
    "#,
    );
    assert_eq!(
        json(&host, &format!(r#"table("t").get("{id}")"#))["a"],
        json!(9)
    );
}

#[test]
fn retried_new_record_save_reuses_its_id() {
    let caps = VistaCapabilities {
        can_insert: true,
        ..VistaCapabilities::default()
    };
    let shell = SpyShell::new(mock_with("r1", &[], caps));
    shell.lose_next_insert.store(true, Ordering::SeqCst);
    let host = host_over(shell);
    let out = json(
        &host,
        r#"
        let r = table("t").record();
        r.a = 5;
        let first = "saved";
        try {
            r.save();
        } catch(err) {
            first = r.status();
        }
        let id = r.save();
        #{ first: first, id: id, ids: table("t").ids() }
    "#,
    );
    assert_eq!(out["first"], json!("failed"), "{out}");
    assert_eq!(out["ids"], json!(["r1", out["id"]]), "{out}");
}

#[test]
fn unchanged_save_writes_nothing() {
    // can_update is false; if save() attempted a patch despite no staged
    // changes, the capability check inside `target()` would throw.
    let caps = VistaCapabilities {
        can_update: false,
        ..VistaCapabilities::default()
    };
    let host = host_over(mock_with(
        "r1",
        &[("n", CborValue::Integer(3.into()))],
        caps,
    ));
    let id: String = eval(
        &host,
        r#"
        let r = table("t").record("r1");
        r.save()
    "#,
    );
    assert_eq!(id, "r1");
}

#[test]
fn loaded_record_saves_only_changed_fields() {
    let shell = SpyShell::new(mock_with(
        "r1",
        &[
            ("a", CborValue::Integer(1.into())),
            ("b", CborValue::Integer(2.into())),
        ],
        VistaCapabilities {
            can_update: true,
            ..VistaCapabilities::default()
        },
    ));
    let patches = shell.patches.clone();
    let host = host_over(shell);
    let _ = run(
        &host,
        r#"
        let r = table("t").record("r1");
        r.a = 99;
        r.save();
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
        let id = r.save();
        table("t").get(id)
    "#,
    );
    assert_eq!(out["inventory"]["stock"], json!(12));
    assert_eq!(out["name"], json!("Widget"));
}

#[test]
fn revert_field_and_all() {
    let host = read_write(&store());
    let dirty_a: bool = eval(
        &host,
        r#"
        let r = table("t").record("r1");
        r.a = 1;
        r.b = 2;
        r.revert("a");
        r.dirty("a")
    "#,
    );
    assert!(!dirty_a, "reverting one field must clear its dirty flag");

    let still_dirty: bool = eval(
        &host,
        r#"
        let r = table("t").record("r1");
        r.a = 1;
        r.b = 2;
        r.revert("a");
        r.is_dirty()
    "#,
    );
    assert!(
        still_dirty,
        "the other staged field survives a single revert"
    );

    let clean: bool = eval(
        &host,
        r#"
        let r = table("t").record("r1");
        r.a = 1;
        r.b = 2;
        r.revert();
        r.is_dirty()
    "#,
    );
    assert!(
        !clean,
        "revert() with no argument clears every staged field"
    );
}

#[test]
fn failed_save_sets_status_and_rejection_and_throws() {
    let host = read_write(&store());
    let out = json(
        &host,
        r#"
        let r = table("t").record("r1");
        r.a = 42;
        table("t").delete("r1");
        let threw = false;
        try {
            r.save();
        } catch(err) {
            threw = true;
        }
        #{ threw: threw, status: r.status(), rejection: r.rejection() }
    "#,
    );
    assert_eq!(out["threw"], json!(true));
    assert_eq!(out["status"], json!("failed"));
    assert!(
        out["rejection"]["message"].as_str().unwrap().contains("r1"),
        "{out}"
    );
}

#[test]
fn setting_id_column_errors() {
    let host = read_write(&store());
    let err = run(
        &host,
        r#"
        let r = table("t").record("r1");
        r.id = "nope";
    "#,
    )
    .unwrap_err();
    assert!(err.contains("id"), "{err}");

    let err = run(
        &host,
        r#"
        let r = table("t").record("r1");
        r["id"] = "nope";
    "#,
    )
    .unwrap_err();
    assert!(err.contains("id"), "{err}");
}

#[test]
fn record_on_missing_id_errors() {
    let host = read_write(&store());
    let err = run(&host, r#"table("t").record("missing")"#).unwrap_err();
    assert!(
        err.contains("not found") || err.contains("missing"),
        "{err}"
    );
}

#[test]
fn denied_writes_block_save() {
    let host = host(&store(), Terminals::Read { limit: None });
    let err = run(
        &host,
        r#"
        let r = table("t").record("r1");
        r.a = 1;
        r.save()
    "#,
    )
    .unwrap_err();
    assert!(err.contains("writes aren't available here"), "{err}");

    let deleted = run(
        &host,
        r#"
        let r = table("t").record("r1");
        r.delete()
    "#,
    )
    .unwrap();
    assert!(!deleted.as_bool().unwrap(), "a denied delete is `false`");
}

#[test]
fn denied_delete_leaves_status_unchanged() {
    let host = host(&store(), Terminals::Read { limit: None });
    let out = json(
        &host,
        r#"
        let r = table("t").record("r1");
        r.a = 1;
        try { r.save(); } catch(err) {}
        let before = r.status();
        let deleted = r.delete();
        #{ before: before, after: r.status(), deleted: deleted }
    "#,
    );
    assert_eq!(out["deleted"], json!(false), "{out}");
    assert_eq!(out["after"], out["before"], "{out}");
}

#[test]
fn field_sets_resolve_the_id_column_once() {
    let shell = mock_with("r1", &[], VistaCapabilities::default());
    let calls = Arc::new(Mutex::new(0usize));
    let seen = calls.clone();
    let resolver: TargetResolver = Arc::new(move |name: &str| {
        *seen.lock().unwrap() += 1;
        Ok(Vista::new(name, Box::new(shell.clone())))
    });
    let host = Host::builder(Limits::background())
        .vocab(DataVocab::read_write(Some(resolver)))
        .build();
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
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_id_column("id");
    MockShell::new()
        .with_metadata(metadata)
        .with_capabilities(caps)
        .with_record(id, record)
}

/// A `ReadWrite`, unlimited host over a single fixed shell — every
/// `table(name)` call resolves to the same shell regardless of `name`.
fn host_over(shell: impl TableShell + Clone + 'static) -> Host {
    let resolver: TargetResolver =
        Arc::new(move |name: &str| Ok(Vista::new(name, Box::new(shell.clone()))));
    Host::builder(Limits::background())
        .vocab(DataVocab::read_write(Some(resolver)))
        .build()
}

/// Wraps a `MockShell` and records every `partial` a `patch_vista_value`
/// call carries, so a test can assert on exactly what a record's `save()`
/// sent — not just the final stored state, which a full-record patch would
/// produce identically (`MockShell::patch_vista_value` merges either way).
/// With `lose_next_insert` set, the next insert lands but reports an error,
/// the way a write whose response is lost does.
#[derive(Clone)]
struct SpyShell {
    inner: MockShell,
    patches: Arc<Mutex<Vec<Record<CborValue>>>>,
    lose_next_insert: Arc<AtomicBool>,
}

impl SpyShell {
    fn new(inner: MockShell) -> Self {
        Self {
            inner,
            patches: Arc::default(),
            lose_next_insert: Arc::default(),
        }
    }
}

#[async_trait]
impl TableShell for SpyShell {
    fn columns(&self) -> &IndexMap<String, Column> {
        self.inner.columns()
    }

    fn references(&self) -> &IndexMap<String, Reference> {
        self.inner.references()
    }

    fn id_column(&self) -> Option<&str> {
        self.inner.id_column()
    }

    async fn list_vista_values(
        &self,
        vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.inner.list_vista_values(vista).await
    }

    async fn get_vista_value(
        &self,
        vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        self.inner.get_vista_value(vista, id).await
    }

    async fn get_vista_some_value(
        &self,
        vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        self.inner.get_vista_some_value(vista).await
    }

    async fn patch_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.patches.lock().unwrap().push(partial.clone());
        self.inner.patch_vista_value(vista, id, partial).await
    }

    async fn insert_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let stored = self.inner.insert_vista_value(vista, id, record).await?;
        if self.lose_next_insert.swap(false, Ordering::SeqCst) {
            return Err(vantage_core::error!("connection reset"));
        }
        Ok(stored)
    }

    fn capabilities(&self) -> &VistaCapabilities {
        self.inner.capabilities()
    }

    fn preview_query(&self, vista: &Vista) -> serde_json::Value {
        self.inner.preview_query(vista)
    }
}
