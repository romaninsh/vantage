//! Vantage's two threads, in miniature: a backend host computes on a worker
//! thread while a UI host formats on this one. The scripts never call each
//! other — a shared `globals` is the whole interface — and what each script
//! may do with it is decided by the vocabulary its host was built with.
//!
//! `cargo run --example two_hosts`

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use vantage_rhai::rhai::{Dynamic, Engine};
use vantage_rhai::{Block, Env, Host, Limits, Template, Vocab};

#[derive(serde::Deserialize)]
struct Defs {
    compute: Block,
    label: Template,
}

/// The shared context. `globals.pi` reaches the map through rhai's indexer,
/// which is what `.prop` falls back to when no getter of that name exists.
#[derive(Clone)]
struct Globals(Arc<Mutex<HashMap<String, f64>>>);

/// Read access. Both hosts get this.
struct Readable;

impl Vocab for Readable {
    fn register(&self, engine: &mut Engine) {
        engine.register_type_with_name::<Globals>("Globals");
        engine.register_indexer_get(|g: &mut Globals, key: &str| {
            g.0.lock().unwrap().get(key).copied().unwrap_or(f64::NAN)
        });
    }
}

/// Write access plus the frame boundary. Only the backend host gets this, so
/// `globals.x = 1` on the UI host fails for want of a setter.
///
/// Writes are silent and `commit` is what asks for a repaint, so a batch that
/// sets several values redraws once against all of them rather than being
/// read halfway through.
struct Writable(mpsc::Sender<()>);

impl Vocab for Writable {
    fn register(&self, engine: &mut Engine) {
        engine.register_indexer_set(|g: &mut Globals, key: &str, value: f64| {
            g.0.lock().unwrap().insert(key.to_string(), value);
        });
        let tx = self.0.clone();
        engine.register_fn("commit", move || {
            let _ = tx.send(());
        });
    }
}

/// Formatting. Only the UI host gets this.
struct Format;

impl Vocab for Format {
    fn register(&self, engine: &mut Engine) {
        engine.register_fn("fixed", |v: f64, places: i64| {
            format!("{v:.*}", places as usize)
        });
    }
}

fn main() -> Result<(), vantage_rhai::RhaiError> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/two_hosts.yaml");
    let yaml = std::fs::read_to_string(path).expect("the example ships its own YAML");
    let Defs { compute, label } = serde_yaml_ng::from_str(&yaml).expect("that YAML parses");

    let globals = Globals(Arc::new(Mutex::new(HashMap::new())));
    let (tx, repaint) = mpsc::channel();
    let env = Env::new().var("globals", Dynamic::from(globals.clone()));

    let ui = Host::builder(Limits::Ui)
        .vocab(Readable)
        .vocab(Format)
        .build();
    let label = ui.compile(&label)?;

    let worker_env = env.clone();
    std::thread::spawn(move || {
        let backend = Host::builder(Limits::background())
            .vocab(Readable)
            .vocab(Writable(tx))
            .build();
        backend
            .compile(&compute)
            .and_then(|script| script.run(&worker_env))
            .expect("the compute script runs")
    });

    // One repaint per write. The sender lives only in the backend host, so
    // the channel closes when the worker drops it and the last line printed
    // is the final estimate.
    for () in repaint {
        println!("{}", label.eval_as::<String>(&env)?);
    }
    println!("pi = 3.141593 — compare to see which digits had settled");
    Ok(())
}
