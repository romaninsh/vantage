//! Two engines, two threads: a backend host computes while a UI host formats.
//!
//! Where `invalidation` shows the mechanics inside one engine, this is the
//! contract between two. They share a scope and nothing else, and each host's
//! vocabulary decides what its script may do with it: only the backend was
//! given a setter, only the UI was given formatting.
//!
//! It mirrors `ui-scope`: every path is its own source carrying a generation,
//! a write bumps that generation *only when the value differs*, and a pump
//! loop on the reading side repaints whatever moved since the last frame.
//! Nothing tells the UI to redraw — it notices.
//!
//! `cargo run --example two_hosts`

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vantage_rhai::rhai::{Dynamic, Engine};
use vantage_rhai::{Block, Compiled, Env, Host, Limits, Lookup, Resolver, Template, Vocab};

#[derive(serde::Deserialize)]
struct Defs {
    compute: Block,
    labels: BTreeMap<String, Template>,
}

/// One scope entry. The generation moves only when the value does, so a
/// producer that recomputes the same answer notifies nobody — the dedup
/// `ui-scope`'s components spell out as `if source.get() != current`.
#[derive(Default)]
struct Source {
    value: Mutex<f64>,
    generation: AtomicU64,
}

impl Source {
    fn set(&self, new: f64) {
        let mut held = self.value.lock().unwrap();
        if *held == new {
            return;
        }
        *held = new;
        self.generation.fetch_add(1, Ordering::Release);
    }

    fn get(&self) -> f64 {
        *self.value.lock().unwrap()
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

/// The shared scope: one source per path, declared up front the way a page's
/// variables are. Discovery needs them to exist before it can resolve a read.
#[derive(Clone, Default)]
struct Scope(Arc<Mutex<HashMap<String, Arc<Source>>>>);

impl Scope {
    fn declare(&self, keys: &[&str]) {
        let mut map = self.0.lock().unwrap();
        for key in keys {
            map.insert(key.to_string(), Arc::new(Source::default()));
        }
    }

    fn source(&self, key: &str) -> Option<Arc<Source>> {
        self.0.lock().unwrap().get(key).cloned()
    }
}

/// The UI reads through a resolver, so every read is recorded and each label
/// ends up knowing its own dependencies.
impl Resolver for Scope {
    fn resolve(&self, path: &str) -> Lookup {
        if path == "globals" {
            return Lookup::Namespace;
        }
        match path.strip_prefix("globals.").and_then(|k| self.source(k)) {
            Some(source) => Lookup::Leaf(Dynamic::from(source.get())),
            None => Lookup::Unknown,
        }
    }
}

/// The backend writes through a handle pushed into its `Env`. In a real app
/// this is a Rust API rather than a script verb; here it keeps both sides
/// speaking the same `globals.<key>` vocabulary.
struct Writable;

impl Vocab for Writable {
    fn register(&self, engine: &mut Engine) {
        engine.register_type_with_name::<Scope>("Scope");
        engine.register_indexer_set(|s: &mut Scope, key: &str, value: f64| {
            if let Some(source) = s.source(key) {
                source.set(value);
            }
        });
    }
}

/// Formatting, and nothing that could block a repaint.
struct Format;

impl Vocab for Format {
    fn register(&self, engine: &mut Engine) {
        engine.register_fn("fixed", |v: f64, places: i64| {
            format!("{v:.*}", places as usize)
        });
    }
}

/// A label plus the generations it last painted, one per path it reads.
struct Label {
    template: Compiled<Template>,
    watched: Vec<(Arc<Source>, u64)>,
}

impl Label {
    fn connect(template: Compiled<Template>, scope: &Scope) -> Label {
        let watched = subscribe(template.read_set(), scope);
        Label { template, watched }
    }

    /// Repaint if any watched path moved. Several writes between two frames
    /// collapse into one repaint, because this compares generations rather
    /// than counting notifications.
    fn pump(&mut self, env: &Env) -> Option<String> {
        let mut moved = false;
        for (source, seen) in &mut self.watched {
            let now = source.generation();
            if now != *seen {
                *seen = now;
                moved = true;
            }
        }
        moved.then(|| self.template.eval_as::<String>(env).expect("label renders"))
    }
}

fn subscribe(read_set: &BTreeSet<String>, scope: &Scope) -> Vec<(Arc<Source>, u64)> {
    read_set
        .iter()
        .filter_map(|path| scope.source(path.strip_prefix("globals.")?))
        .map(|source| {
            let seen = source.generation();
            (source, seen)
        })
        .collect()
}

fn main() -> Result<(), vantage_rhai::RhaiError> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/two_hosts.yaml");
    let yaml = std::fs::read_to_string(path).expect("the example ships its own YAML");
    let Defs { compute, labels } = serde_yaml_ng::from_str(&yaml).expect("that YAML parses");

    let scope = Scope::default();
    scope.declare(&["batch", "pi"]);

    let ui = Host::builder(Limits::Ui).vocab(Format).build();
    let read_env = Env::new().resolver(Arc::new(scope.clone()));
    let mut labels: Vec<(String, Label)> = labels
        .into_iter()
        .map(|(name, template)| {
            let compiled = ui.compile(&template)?.discover(&read_env)?;
            Ok((name, Label::connect(compiled, &scope)))
        })
        .collect::<Result<_, vantage_rhai::RhaiError>>()?;
    for (name, label) in &labels {
        println!("{name:<6} reads {:?}", label.template.read_set());
    }

    let write_env = Env::new().var("globals", Dynamic::from(scope.clone()));
    let worker = std::thread::spawn(move || {
        Host::builder(Limits::background())
            .vocab(Writable)
            .build()
            .compile(&compute)
            .and_then(|script| script.run(&write_env))
            .expect("the compute script runs")
    });

    // The frame clock. `Mount::tick` is this loop behind a real timer.
    println!("\n  repaints:");
    while !worker.is_finished() {
        paint(&mut labels, &read_env);
        std::thread::sleep(Duration::from_millis(2));
    }
    paint(&mut labels, &read_env);

    println!(
        "\n`batch` repainted every time. `pi` fell quiet once its fifth decimal\n\
         stopped moving: same value written, no generation bump, no repaint."
    );
    Ok(())
}

fn paint(labels: &mut [(String, Label)], env: &Env) {
    for (_, label) in labels {
        if let Some(text) = label.pump(env) {
            println!("  {text}");
        }
    }
}
