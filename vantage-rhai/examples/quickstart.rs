//! The README's seven steps, running: three slot kinds loaded from a YAML
//! file, a vocabulary, a resolver, discovery, and what each error looks like.
//!
//! `cargo run --example quickstart`

use std::sync::Arc;

use vantage_rhai::rhai::{Dynamic, Engine};
use vantage_rhai::{Block, Env, Expr, Host, Limits, Lookup, Resolver, Template, Vocab};

/// The field types are the only type information; the YAML is plain scalars.
#[derive(serde::Deserialize)]
struct LineDef {
    when: Option<Expr>,
    label: Template,
    action: Block,
}

#[derive(Clone)]
struct Line {
    qty: i64,
    unit_price: i64,
}

/// Registers the *type*, never one line, so a single host serves every row
/// and the row itself arrives per call through `Env`.
struct LineVocab;

impl Vocab for LineVocab {
    fn register(&self, engine: &mut Engine) {
        engine.register_type_with_name::<Line>("Line");
        engine.register_get("qty", |l: &mut Line| l.qty);
        engine.register_fn("total", |l: &mut Line| l.qty * l.unit_price);
    }
}

/// Stands in for a page scope: one namespace, one leaf, anything else a typo.
struct Settings;

impl Resolver for Settings {
    fn resolve(&self, path: &str) -> Lookup {
        match path {
            "settings" => Lookup::Namespace,
            "settings.currency" => Lookup::Leaf(Dynamic::from("£".to_string())),
            _ => Lookup::Unknown,
        }
    }
}

fn main() -> Result<(), vantage_rhai::RhaiError> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/quickstart.yaml");
    let yaml = std::fs::read_to_string(path).expect("the example ships its own YAML");
    let def: LineDef = serde_yaml_ng::from_str(&yaml).expect("that YAML parses");

    let host = Host::builder(Limits::Ui).vocab(LineVocab).build();
    let line = Line {
        qty: 3,
        unit_price: 250,
    };
    let env = Env::new()
        .var("line", Dynamic::from(line.clone())) // own type: wrap it
        .var("qty", line.qty)
        .resolver(Arc::new(Settings));

    let guard = def.when.as_ref().expect("this YAML declares one");
    println!("guard   -> {}", host.compile(guard)?.eval_bool(&env)?);
    let label = host.compile(&def.label)?;
    println!("label   -> {}", label.eval_as::<String>(&env)?);
    let action = host.compile(&def.action)?;
    println!("action  -> {}", action.eval_as::<i64>(&env)?);
    println!("reads   -> {:?}", label.discover(&env)?.read_set());

    // Every error names its slot; syntax and runtime faults add a position.
    let (typo, unclosed, forever) = (
        Expr::from("settings.currancy"),
        Block::from("let n = 1;\nrow.save()\nlet m = 2;\n"),
        Block::from("while true {}"),
    );
    let e = host.compile(&typo)?.eval(&env).unwrap_err();
    println!("typo    -> {e} (in `{}`)", e.src());
    println!("syntax  -> {}", host.compile(&unclosed).unwrap_err());
    let e = host.compile(&forever)?.run(&Env::new()).unwrap_err();
    println!("runaway -> {e}");
    Ok(())
}
