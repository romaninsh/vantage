//! The seven steps from the README, as code that runs.
//!
//! An invoice line with a `when:` guard, a `label:` template and an
//! `action:` body — the three slot kinds — over a vocabulary of one verb
//! and a resolver standing in for a page scope.
//!
//! `cargo run --example quickstart`

use std::sync::Arc;

use vantage_rhai::rhai::{Dynamic, Engine};
use vantage_rhai::{Block, Env, Expr, Host, Limits, Lookup, Resolver, Template, Vocab};

/// Step 2: the YAML struct. Each field's type says what an author may write
/// in it; `serde(transparent)` on the slots means the YAML is a plain scalar.
#[derive(serde::Deserialize)]
struct LineDef {
    when: Option<Expr>,
    label: Template,
    action: Block,
}

/// Step 4: a vocabulary registers a *type* and its methods, never one
/// instance — so a single host serves every line, and the line itself
/// arrives through `Env`.
#[derive(Clone)]
struct Line {
    qty: i64,
    unit_price: i64,
}

struct LineVocab;

impl Vocab for LineVocab {
    fn register(&self, engine: &mut Engine) {
        engine.register_type_with_name::<Line>("Line");
        engine.register_get("qty", |l: &mut Line| l.qty);
        engine.register_get("unit_price", |l: &mut Line| l.unit_price);
        engine.register_fn("total", |l: &mut Line| l.qty * l.unit_price);
    }
}

/// A stand-in for a page scope: `settings.currency` resolves, `settings`
/// alone is a namespace to descend, anything else is a typo.
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
    let def: LineDef = serde_yaml_ng::from_str(
        r#"
when: qty > 0
label: '${ settings.currency }${ line.total() } for ${ line.qty }'
action: |
  let discounted = line.total() * 9 / 10;
  discounted
"#,
    )
    .expect("the example's own YAML parses");

    // Step 3 and 5: one profile, one host, built once.
    let host = Host::builder(Limits::Ui).vocab(LineVocab).build();
    let guard = def.when.as_ref().map(|e| host.compile(e)).transpose()?;
    let label = host.compile(&def.label)?;
    let action = host.compile(&def.action)?;

    // Step 6: a fresh `Env` per call — the pushed line, the lazy scope.
    let line = Line {
        qty: 3,
        unit_price: 250,
    };
    let env = Env::new()
        .var("line", Dynamic::from(line.clone()))
        .var("qty", line.qty)
        .resolver(Arc::new(Settings));

    if let Some(guard) = &guard {
        println!("guard      -> {}", guard.eval_bool(&env)?);
    }
    println!("label      -> {}", label.eval_as::<String>(&env)?);
    println!("action     -> {}", action.eval_as::<i64>(&env)?);

    // Discovery freezes what the script read through the resolver, which is
    // the dependency set a UI would subscribe to.
    let discovered = host.compile(&def.label)?.discover(&env)?;
    println!("label reads-> {:?}", discovered.read_set());

    // Step 7: every error carries the slot source, so a validator can name
    // the YAML key. A typo in a scope path fails rather than reading unit.
    let typo = host.compile(&Expr::from("settings.currancy"))?;
    match typo.eval(&env) {
        Err(e) => println!("typo       -> {e} (in `{}`)", e.src()),
        Ok(v) => unreachable!("an unknown name must not resolve, got {v}"),
    }

    // A syntax fault additionally points at the line and column.
    match host.compile(&Block::from("let n = 1;\nrow.save()\nlet m = 2;\n")) {
        Err(e) => println!("syntax     -> {e}"),
        Ok(_) => unreachable!("the missing semicolon must not parse"),
    }

    // A runaway script stops rather than hanging the caller.
    match host
        .compile(&Block::from("while true {}"))?
        .run(&Env::new())
    {
        Err(e) => println!("runaway    -> {e}"),
        Ok(()) => unreachable!("the UI profile must bound this loop"),
    }

    Ok(())
}
