use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use vantage_rhai::rhai::{Dynamic, Map, NativeCallContext};
use vantage_rhai::{Block, Env, Host, Limits, Lookup, ModuleDir, Resolver, RhaiError};

/// A fresh `lib/` folder per test, removed on drop.
struct Lib(PathBuf);

impl Lib {
    fn new(test: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "vantage-rhai-modules-{}-{test}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Lib(root)
    }

    fn write(&self, name: &str, src: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, src).unwrap();
        path
    }
}

impl Drop for Lib {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn host(dir: &ModuleDir) -> Host {
    Host::builder(Limits::Ui).modules(dir).build()
}

fn eval(host: &Host, env: &Env, src: &str) -> Result<Dynamic, RhaiError> {
    host.compile(&Block::from(src))?.eval(env)
}

fn map(pairs: &[(&str, &str)]) -> Map {
    pairs
        .iter()
        .map(|(k, v)| ((*k).into(), Dynamic::from(v.to_string())))
        .collect()
}

struct SessionResolver;

impl Resolver for SessionResolver {
    fn resolve(&self, path: &str) -> Lookup {
        match path {
            "secret" => Lookup::Leaf(Dynamic::from("hidden".to_string())),
            _ => Lookup::Unknown,
        }
    }
}

#[test]
fn lib_import_calls_helper() {
    let lib = Lib::new("calls");
    lib.write(
        "plans.rhai",
        r#"fn can_edit(plan, session) { plan.owner == session.owner || session.role == "admin" }"#,
    );
    let dir = ModuleDir::new(&lib.0);
    let host = host(&dir);
    let body = r#"import "plans" as plans; plans::can_edit(row, session)"#;

    let env = |owner: &str, role: &str| {
        Env::new()
            .var("row", map(&[("owner", "ann")]))
            .var("session", map(&[("owner", owner), ("role", role)]))
    };
    assert!(
        eval(&host, &env("ann", "user"), body)
            .unwrap()
            .as_bool()
            .unwrap()
    );
    assert!(
        !eval(&host, &env("bob", "user"), body)
            .unwrap()
            .as_bool()
            .unwrap()
    );
    assert!(
        eval(&host, &env("bob", "admin"), body)
            .unwrap()
            .as_bool()
            .unwrap()
    );
}

#[test]
fn lib_import_reaches_subfolders_and_nested_imports() {
    let lib = Lib::new("nested");
    lib.write("checks/kind.rhai", "fn ok(k) { k == \"goal\" }");
    lib.write(
        "plans.rhai",
        "import \"checks/kind\" as kind;\nfn fits(k) { kind::ok(k) }",
    );
    let host = host(&ModuleDir::new(&lib.0));

    let out = eval(
        &host,
        &Env::new(),
        r#"import "plans" as p; [p::fits("goal"), p::fits("task")]"#,
    )
    .unwrap();
    let out: Vec<bool> = out
        .into_array()
        .unwrap()
        .into_iter()
        .map(|v| v.as_bool().unwrap())
        .collect();
    assert_eq!(out, vec![true, false]);
}

#[test]
fn lib_helper_sees_only_arguments() {
    let lib = Lib::new("isolated");
    lib.write(
        "leak.rhai",
        "fn pushed() { session }\nfn resolved() { secret }",
    );
    let host = host(&ModuleDir::new(&lib.0));
    let env = Env::new()
        .var("session", map(&[("role", "admin")]))
        .resolver(Arc::new(SessionResolver));

    // The body itself sees both.
    assert_eq!(
        eval(&host, &env, "secret").unwrap().into_string().unwrap(),
        "hidden"
    );
    for (call, name) in [("l::pushed()", "session"), ("l::resolved()", "secret")] {
        let src = format!("import \"leak\" as l; {call}");
        let err = eval(&host, &env, &src).unwrap_err();
        assert!(
            matches!(&err, RhaiError::UnknownName { path, .. } if path == name),
            "{call}: {err}"
        );
    }
}

#[test]
fn lib_helper_call_is_told_apart_from_body_call() {
    let lib = Lib::new("verb");
    lib.write("inner.rhai", "fn t() { verb() }");
    lib.write("outer.rhai", "import \"inner\" as i;\nfn t() { i::t() }");
    let host = Host::builder(Limits::Ui)
        .modules(&ModuleDir::new(&lib.0))
        .vocab_fn(|e| {
            e.register_fn("verb", |ctx: NativeCallContext| {
                ModuleDir::is_helper_call(&ctx)
            });
        })
        .build();

    let out = eval(
        &host,
        &Env::new(),
        r#"import "outer" as o; [verb(), o::t()]"#,
    )
    .unwrap();
    let out: Vec<bool> = out
        .into_array()
        .unwrap()
        .into_iter()
        .map(|v| v.as_bool().unwrap())
        .collect();
    assert_eq!(out, vec![false, true]);
}

#[test]
fn lib_module_top_level_cannot_see_caller_scope() {
    let lib = Lib::new("toplevel");
    lib.write("grab.rhai", "export const role = session.role;");
    let host = host(&ModuleDir::new(&lib.0));
    let env = Env::new().var("session", map(&[("role", "admin")]));

    let err = eval(&host, &env, r#"import "grab" as g; g::role"#)
        .unwrap_err()
        .to_string();
    assert!(err.contains("session"), "{err}");
}

#[test]
fn lib_import_refuses_paths_outside_the_folder() {
    let outer = Lib::new("outside");
    outer.write("secret.rhai", "fn x() { 1 }");
    let lib = Lib::new("outside-lib");
    let host = host(&ModuleDir::new(&lib.0));
    let escape = format!(
        "../{}/secret",
        outer.0.file_name().unwrap().to_string_lossy()
    );
    let absolute = outer.0.join("secret").to_string_lossy().into_owned();

    for name in [escape.as_str(), absolute.as_str(), "", "a//b", "plans.rhai"] {
        let src = format!("import {name:?} as s; s::x()");
        let err = eval(&host, &Env::new(), &src).unwrap_err().to_string();
        assert!(err.contains("lib/"), "{name}: {err}");
    }
}

#[test]
fn lib_import_cycle_is_an_error_not_a_crash() {
    let lib = Lib::new("cycle");
    lib.write("a.rhai", "import \"b\" as b;\nfn x() { 1 }");
    lib.write("b.rhai", "import \"a\" as a;\nfn y() { 2 }");
    let host = host(&ModuleDir::new(&lib.0));

    let err = eval(&host, &Env::new(), r#"import "a" as a; a::x()"#)
        .unwrap_err()
        .to_string();
    assert!(err.contains("cycle"), "{err}");
}

#[test]
fn import_without_module_dir_is_refused() {
    let host = Host::builder(Limits::Ui).build();
    let err = eval(&host, &Env::new(), r#"import "Cargo" as c; 1"#)
        .unwrap_err()
        .to_string();
    assert!(err.contains("Cargo"), "{err}");
}

#[test]
fn lib_change_reloads_module_and_notifies_dependents() {
    let lib = Lib::new("reload");
    lib.write("base.rhai", "fn n() { 1 }");
    lib.write(
        "mid.rhai",
        "import \"base\" as base;\nfn n() { base::n() * 10 }",
    );
    lib.write("other.rhai", "fn n() { 7 }");
    let dir = ModuleDir::new(&lib.0);
    let host = host(&dir);
    let seen: Arc<Mutex<Vec<BTreeSet<String>>>> = Arc::default();
    let sink = seen.clone();
    dir.on_reload(move |names| sink.lock().unwrap().push(names.clone()));

    let body = host
        .compile(&Block::from(
            r#"import "mid" as m; import "other" as o; m::n() + o::n()"#,
        ))
        .unwrap();
    assert_eq!(
        body.imports(),
        BTreeSet::from(["mid".to_string(), "other".to_string()])
    );
    assert_eq!(body.eval(&Env::new()).unwrap().as_int().unwrap(), 17);

    // Cached until told: an edit alone changes nothing.
    let base = lib.write("base.rhai", "fn n() { 2 }");
    assert_eq!(body.eval(&Env::new()).unwrap().as_int().unwrap(), 17);

    let affected = dir.changed(&base);
    let expected = BTreeSet::from(["base".to_string(), "mid".to_string()]);
    assert_eq!(affected, expected);
    assert_eq!(*seen.lock().unwrap(), vec![expected]);
    assert_eq!(body.eval(&Env::new()).unwrap().as_int().unwrap(), 27);

    assert!(dir.changed(&lib.0.join("notes.txt")).is_empty());
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn a_change_racing_a_load_caches_nothing_stale() {
    let lib = Lib::new("race");
    let racy = lib.write("racy.rhai", "edit();\nfn n() { 1 }");
    let dir = ModuleDir::new(&lib.0);
    let watcher = dir.clone();
    // The module's top level edits its own file and reports it, after the
    // load read the old text and before it is cached.
    let host = Host::builder(Limits::Ui)
        .modules(&dir)
        .vocab_fn(move |e| {
            let (watcher, racy) = (watcher.clone(), racy.clone());
            e.register_fn("edit", move || {
                fs::write(&racy, "fn n() { 2 }").unwrap();
                watcher.changed(&racy);
            });
        })
        .build();
    let body = r#"import "racy" as r; r::n()"#;

    assert_eq!(eval(&host, &Env::new(), body).unwrap().as_int().unwrap(), 1);
    assert_eq!(eval(&host, &Env::new(), body).unwrap().as_int().unwrap(), 2);
}
