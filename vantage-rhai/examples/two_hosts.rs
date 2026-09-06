//! Two engines on two threads, sharing a scope and nothing else. Where
//! `invalidation` shows the mechanics inside one engine, this is the contract
//! between two, mirroring `ui-scope`: a write bumps a generation only when the
//! value differs, and a pump loop repaints whatever moved since the last
//! frame. Nothing tells the UI to redraw. It notices.
//!
//! `cargo run --example two_hosts`

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vantage_rhai::rhai::{Dynamic, Engine};
use vantage_rhai::{Block, Compiled, Env, Host, Limits, Lookup, Resolver, Template, Vocab};

#[derive(serde::Deserialize)]
struct Defs {
    compute: Block,
    labels: BTreeMap<String, Template>,
}

/// A value and the generation of its last *change*. Writing the same value
/// again moves nothing — the dedup `ui-scope` spells out at each call site as
/// `if source.get() != current`.
#[derive(Default)]
struct Source(Mutex<(f64, u64)>);

impl Source {
    fn set(&self, value: f64) {
        let mut held = self.0.lock().unwrap();
        if held.0 != value {
            *held = (value, held.1 + 1);
        }
    }

    fn read(&self) -> (f64, u64) {
        *self.0.lock().unwrap()
    }
}

/// The shared scope: one source per path, so labels wake independently.
#[derive(Clone, Default)]
struct Scope {
    batch: Arc<Source>,
    pi: Arc<Source>,
}

impl Scope {
    fn key(&self, key: &str) -> Option<&Arc<Source>> {
        match key {
            "batch" => Some(&self.batch),
            "pi" => Some(&self.pi),
            _ => None,
        }
    }
}

/// The UI reads through a resolver, so discovery records what each label
/// depends on.
impl Resolver for Scope {
    fn resolve(&self, path: &str) -> Lookup {
        if path == "globals" {
            return Lookup::Namespace;
        }
        match path.strip_prefix("globals.").and_then(|k| self.key(k)) {
            Some(source) => Lookup::Leaf(Dynamic::from(source.read().0)),
            None => Lookup::Unknown,
        }
    }
}

/// Write access, given only to the backend host — so the UI script cannot
/// assign to `globals`, the operation is not there to call.
struct Writable;

impl Vocab for Writable {
    fn register(&self, engine: &mut Engine) {
        engine.register_type_with_name::<Scope>("Scope");
        engine.register_indexer_set(|s: &mut Scope, key: &str, value: f64| {
            if let Some(source) = s.key(key) {
                source.set(value);
            }
        });
    }
}

/// Formatting. Only the UI host is given it.
struct Format;

impl Vocab for Format {
    fn register(&self, engine: &mut Engine) {
        engine.register_fn("fixed", |v: f64, places: i64| {
            format!("{v:.*}", places as usize)
        });
    }
}

/// A label, and the generation it last painted for each path it reads.
struct Label {
    template: Compiled<Template>,
    watched: Vec<(Arc<Source>, u64)>,
}

impl Label {
    /// Repaint if a watched path moved. Comparing generations rather than
    /// counting notifications is what collapses a burst of writes into one
    /// frame.
    fn pump(&mut self, env: &Env) -> Option<String> {
        let mut moved = false;
        for (source, seen) in &mut self.watched {
            let now = source.read().1;
            if now != *seen {
                *seen = now;
                moved = true;
            }
        }
        moved.then(|| self.template.eval_as::<String>(env).expect("label renders"))
    }
}

fn main() -> Result<(), vantage_rhai::RhaiError> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/two_hosts.yaml");
    let yaml = std::fs::read_to_string(path).expect("the example ships its own YAML");
    let Defs { compute, labels } = serde_yaml_ng::from_str(&yaml).expect("that YAML parses");

    let scope = Scope::default();
    let ui = Host::builder(Limits::Ui).vocab(Format).build();
    let env = Env::new().resolver(Arc::new(scope.clone()));

    let mut labels = labels
        .into_iter()
        .map(|(name, source)| {
            let template = ui.compile(&source)?.discover(&env)?;
            println!("{name:<6} reads {:?}", template.read_set());
            let watched = template
                .read_set()
                .iter()
                .filter_map(|p| scope.key(p.strip_prefix("globals.")?))
                .map(|s| (s.clone(), s.read().1))
                .collect();
            Ok(Label { template, watched })
        })
        .collect::<Result<Vec<_>, vantage_rhai::RhaiError>>()?;

    let write_env = Env::new().var("globals", Dynamic::from(scope));
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
        paint(&mut labels, &env);
        std::thread::sleep(Duration::from_millis(2));
    }
    paint(&mut labels, &env);

    println!(
        "\n`batch` repainted every time. `pi` fell quiet once its fifth decimal\n\
         stopped moving: same value written, no generation bump, no repaint."
    );
    Ok(())
}

fn paint(labels: &mut [Label], env: &Env) {
    for label in labels {
        if let Some(text) = label.pump(env) {
            println!("  {text}");
        }
    }
}
