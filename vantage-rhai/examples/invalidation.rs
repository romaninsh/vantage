//! Read-sets decide what to recompute. Both slots roll a random number each
//! time they run, so a number that moved is a slot that re-ran — the
//! randomness is the instrument, not the subject.
//!
//! `cargo run --example invalidation`

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use vantage_rhai::rhai::{Dynamic, Engine};
use vantage_rhai::{Compiled, Env, Expr, Host, Limits, Lookup, Resolver, RhaiError, Vocab};

/// Shared process state in a vocabulary is fine — a clock, a counter, this
/// generator. Per-call data is not; that belongs in the `Env`.
struct RollVocab(Arc<AtomicU64>);

impl Vocab for RollVocab {
    fn register(&self, engine: &mut Engine) {
        let state = self.0.clone();
        engine.register_fn("roll", move || -> f64 {
            let x = state
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |mut x| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    Some(x)
                })
                .expect("the update never declines");
            ((x >> 11) as f64) / ((1u64 << 53) as f64)
        });
    }
}

/// `global` descends, `global.<key>` is a leaf from a shared map — so
/// writing to the map is what a control on the page would do.
#[derive(Clone)]
struct Globals(Arc<Mutex<HashMap<String, f64>>>);

impl Resolver for Globals {
    fn resolve(&self, path: &str) -> Lookup {
        if path == "global" {
            return Lookup::Namespace;
        }
        match path
            .strip_prefix("global.")
            .and_then(|k| self.0.lock().unwrap().get(k).copied())
        {
            Some(v) => Lookup::Leaf(Dynamic::from(v)),
            None => Lookup::Unknown,
        }
    }
}

type Page = Vec<(String, Compiled<Expr>, f64)>;

/// One invalidation pass: recompute whoever read the path that moved. A real
/// page indexes path → slots instead of scanning; the decision is the same.
fn bump(
    page: &mut Page,
    globals: &Globals,
    key: &str,
    to: f64,
    env: &Env,
) -> Result<(), RhaiError> {
    globals.0.lock().unwrap().insert(key.to_string(), to);
    println!("\nglobal.{key} -> {to}");
    for (name, script, value) in page {
        let reads_it = script.read_set().contains(&format!("global.{key}"));
        if reads_it {
            *value = script.eval_as::<f64>(env)?;
        }
        let verdict = if reads_it { "recomputed" } else { "held" };
        println!("  {name:<7}= {value:.4}   {verdict}");
    }
    Ok(())
}

fn main() -> Result<(), RhaiError> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/invalidation.yaml");
    let yaml = std::fs::read_to_string(path).expect("the example ships its own YAML");
    // Sorted, so the seeded roll sequence is identical on every run.
    let mut slots: Vec<(String, Expr)> = serde_yaml_ng::from_str::<HashMap<String, Expr>>(&yaml)
        .expect("that YAML parses")
        .into_iter()
        .collect();
    slots.sort_by(|a, b| a.0.cmp(&b.0));

    // `unrelated` resolves but no slot reads it — the last bump needs that.
    let globals = Globals(Arc::new(Mutex::new(HashMap::from([
        ("increment".to_string(), 1.0),
        ("unrelated".to_string(), 0.0),
    ]))));
    let host = Host::builder(Limits::Ui)
        .vocab(RollVocab(Arc::new(AtomicU64::new(0x2545_F491_4F6C_DD1D))))
        .build();
    let env = Env::new().resolver(Arc::new(globals.clone()));

    // Discovery yields the dependency set and the first value together.
    let mut page = Page::new();
    for (name, expr) in slots {
        let (script, first) = host.compile(&expr)?.discover_value(&env)?;
        let value = first.as_float().expect("both slots evaluate to a number");
        println!("{name:<7}= {value:.4}   reads {:?}", script.read_set());
        page.push((name, script, value));
    }

    bump(&mut page, &globals, "increment", 2.0, &env)?;
    bump(&mut page, &globals, "increment", 3.0, &env)?;
    bump(&mut page, &globals, "unrelated", 99.0, &env)?;

    println!(
        "\n`drift` reads nothing, so nothing invalidates it. Read-sets are\n\
         path-precise: a sibling under `global` moving recomputes neither.\n\
         A read inside a branch discovery did not take is absent from the\n\
         set, so treat it as a floor."
    );
    Ok(())
}
