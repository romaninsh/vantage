//! Scripted mutation — a [`FakerEffect`] whose behavior is a Rhai script.
//!
//! The script runs once per `interval` tick against the shared store, with a
//! `tick` counter in scope. Every mutation verb writes the store **and**
//! broadcasts the matching `ChangeEvent`, so the script literally is
//! "perform change, and it's being sent up". Seeding stays declarative
//! (`count` rows via `ValueGen` before the loop starts) — scripts describe
//! only *change*.
//!
//! Verbs: `ids()`, `get(id)`, `count()` read the store; `set(id, field, v)`,
//! `patch(id, #{…})`, `insert(#{…})`, `delete(id)` mutate and broadcast;
//! `fake(kind)`, `rand_int(a,b)`, `rand_float(a,b)`, `pick(array)` generate.
//!
//! The script compiles once, on a [`vantage_rhai::Host`] carrying the verbs
//! as [`FakerVocab`] under the background limit profile; each tick's
//! evaluation runs inside `spawn_blocking` against that compiled script (a
//! runaway loop in a tick used to hang a blocking thread forever). A script
//! that fails stops the loop after logging once: a static script that failed
//! will fail every tick, and a sim whose data stops moving is a louder,
//! cheaper signal than an error log at 50 Hz. A script that does not even
//! compile stops it before the first tick.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use tokio::time::interval;
use vantage_rhai::rhai;
use vantage_rhai::rhai::{Dynamic, Engine, Map as RhaiMap};
use vantage_rhai::{Block, Compiled, Env, Host, Limits, Vocab};
use vantage_types::Record;

use crate::effect::{FakerCtx, FakerEffect};

/// Seed `count` rows, then run `script` every `interval`.
pub struct RhaiEffect {
    pub count: usize,
    pub interval: Duration,
    pub script: String,
}

#[async_trait]
impl FakerEffect for RhaiEffect {
    fn seed(&self, ctx: &FakerCtx) {
        for _ in 0..self.count {
            ctx.seed_one();
        }
    }

    fn is_live(&self) -> bool {
        true
    }

    async fn run(&self, ctx: Arc<FakerCtx>) {
        // Compile once. A script that does not parse never ticks.
        let script = match compile_effect(&ctx, &self.script) {
            Ok(script) => Arc::new(script),
            Err(e) => {
                tracing::error!(error = %e, "rhai faker effect failed to compile; not starting its mutation loop");
                return;
            }
        };
        let mut ticker = interval(self.interval);
        let mut tick: i64 = 0;
        loop {
            ticker.tick().await;
            let script = script.clone();
            let result = tokio::task::spawn_blocking(move || run_compiled(&script, tick)).await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::error!(error = %e, tick, "rhai faker effect failed; stopping its mutation loop");
                    return;
                }
                Err(e) => {
                    tracing::error!(error = %e, tick, "rhai faker effect panicked; stopping its mutation loop");
                    return;
                }
            }
            tick += 1;
        }
    }
}

/// The mutation verbs, bound to one store, as a [`Vocab`].
pub struct FakerVocab(pub Arc<FakerCtx>);

impl Vocab for FakerVocab {
    fn register(&self, engine: &mut Engine) {
        register_verbs(engine, &self.0);
    }
}

/// Build the host and compile `script` on it, once per effect.
fn compile_effect(
    ctx: &Arc<FakerCtx>,
    script: &str,
) -> std::result::Result<Compiled<Block>, String> {
    let host = Host::builder(Limits::background())
        .vocab(FakerVocab(ctx.clone()))
        .build();
    host.compile_uncached(&Block::from(script))
        .map_err(|e| e.to_string())
}

/// One evaluation of the compiled script with `tick` in scope.
fn run_compiled(script: &Compiled<Block>, tick: i64) -> std::result::Result<(), String> {
    script
        .run(&Env::new().var("tick", tick))
        .map_err(|e| e.to_string())
}

/// Compile and run once — the test path. The effect loop compiles once and
/// runs many.
#[cfg(test)]
fn run_tick(ctx: &Arc<FakerCtx>, script: &str, tick: i64) -> std::result::Result<(), String> {
    run_compiled(&compile_effect(ctx, script)?, tick)
}

fn register_verbs(engine: &mut Engine, ctx: &Arc<FakerCtx>) {
    let c = ctx.clone();
    engine.register_fn("ids", move || -> rhai::Array {
        c.record_ids().into_iter().map(Dynamic::from).collect()
    });

    let c = ctx.clone();
    engine.register_fn("get", move |id: &str| -> Dynamic {
        match c.get_record(id) {
            Some(rec) => Dynamic::from_map(record_to_map(&rec)),
            None => Dynamic::UNIT,
        }
    });

    let c = ctx.clone();
    engine.register_fn("count", move || -> i64 { c.record_count() as i64 });

    let c = ctx.clone();
    engine.register_fn("set", move |id: &str, field: &str, value: Dynamic| {
        c.update_field(id, field, dynamic_to_cbor(&value));
    });

    let c = ctx.clone();
    engine.register_fn("patch", move |id: &str, map: RhaiMap| {
        c.patch_record(id, &map_to_record(&map));
    });

    let c = ctx.clone();
    engine.register_fn("insert", move |map: RhaiMap| -> String {
        c.insert_record(map_to_record(&map))
    });

    let c = ctx.clone();
    engine.register_fn("delete", move |id: &str| {
        c.expire(id);
    });

    let c = ctx.clone();
    engine.register_fn("fake", move |kind: &str| -> Dynamic {
        cbor_to_dynamic(&c.fake_value(kind))
    });

    let c = ctx.clone();
    engine.register_fn("rand_int", move |lo: i64, hi: i64| -> i64 {
        c.rand_int(lo, hi)
    });

    let c = ctx.clone();
    engine.register_fn("rand_float", move |lo: f64, hi: f64| -> f64 {
        c.rand_float(lo, hi)
    });

    let c = ctx.clone();
    engine.register_fn("pick", move |arr: rhai::Array| -> Dynamic {
        match c.rand_index(arr.len()) {
            Some(ix) => arr[ix].clone(),
            None => Dynamic::UNIT,
        }
    });
}

// ---- Value round-tripping (the scalar subset scripts touch) ---------------

fn dynamic_to_cbor(v: &Dynamic) -> CborValue {
    if v.is_unit() {
        CborValue::Null
    } else if let Ok(i) = v.as_int() {
        CborValue::Integer(i.into())
    } else if let Ok(f) = v.as_float() {
        CborValue::Float(f)
    } else if let Ok(b) = v.as_bool() {
        CborValue::Bool(b)
    } else {
        CborValue::Text(v.to_string())
    }
}

fn cbor_to_dynamic(v: &CborValue) -> Dynamic {
    match v {
        CborValue::Text(s) => Dynamic::from(s.clone()),
        CborValue::Integer(i) => Dynamic::from(i128::from(*i) as i64),
        CborValue::Float(f) => Dynamic::from(*f),
        CborValue::Bool(b) => Dynamic::from(*b),
        CborValue::Null => Dynamic::UNIT,
        other => Dynamic::from(format!("{other:?}")),
    }
}

fn record_to_map(rec: &Record<CborValue>) -> RhaiMap {
    rec.iter()
        .map(|(k, v)| (k.as_str().into(), cbor_to_dynamic(v)))
        .collect()
}

fn map_to_record(map: &RhaiMap) -> Record<CborValue> {
    let mut rec = Record::new();
    for (k, v) in map {
        rec.insert(k.to_string(), dynamic_to_cbor(v));
    }
    rec
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;
    use vantage_diorama::ChangeEvent;
    use vantage_vista::mocks::MockShell;

    use crate::FakerColumn;
    use crate::value_gen::ValueGen;

    fn ctx() -> (Arc<FakerCtx>, broadcast::Receiver<ChangeEvent>) {
        let (tx, rx) = broadcast::channel(256);
        let columns = vec![
            FakerColumn {
                name: "id".into(),
                ty: "string".into(),
                flags: vec!["id".into()],
            },
            FakerColumn {
                name: "name".into(),
                ty: "string".into(),
                flags: vec![],
            },
            FakerColumn {
                name: "balance".into(),
                ty: "money".into(),
                flags: vec![],
            },
        ];
        let ctx = Arc::new(
            FakerCtx::new(MockShell::new(), tx, columns, "id".into())
                .with_values(ValueGen::seeded(1))
                .with_seed(Some(1)),
        );
        (ctx, rx)
    }

    fn drain(rx: &mut broadcast::Receiver<ChangeEvent>) -> Vec<ChangeEvent> {
        let mut out = Vec::new();
        while let Ok(e) = rx.try_recv() {
            out.push(e);
        }
        out
    }

    #[test]
    fn title_flip_script_updates_and_broadcasts() {
        let (ctx, mut rx) = ctx();
        for _ in 0..3 {
            ctx.seed_one();
        }
        let script = r#"
            let id = pick(ids());
            let n = get(id).name;
            set(id, "name", if tick % 2 == 0 { n.to_upper() } else { n.to_lower() });
        "#;
        run_tick(&ctx, script, 0).unwrap();
        let events = drain(&mut rx);
        assert_eq!(events.len(), 1);
        let ChangeEvent::Updated { id, new } = &events[0] else {
            panic!("expected Updated, got {:?}", events[0]);
        };
        let name = match new.as_ref().unwrap().get("name").unwrap() {
            CborValue::Text(s) => s.clone(),
            other => panic!("expected text name, got {other:?}"),
        };
        assert_eq!(name, name.to_uppercase(), "tick 0 upper-cases");
        assert!(ctx.get_record(id).is_some());
    }

    #[test]
    fn revolving_script_holds_population_steady() {
        let (ctx, mut rx) = ctx();
        for _ in 0..10 {
            ctx.seed_one();
        }
        let script = r#"
            if count() > 10 { delete(pick(ids())) };
            insert(#{ name: fake("name"), balance: rand_float(0.0, 900000.0) / 100.0 });
        "#;
        for tick in 0..5 {
            run_tick(&ctx, script, tick).unwrap();
        }
        // 10 seeds → first tick inserts (11), later ticks delete+insert.
        assert_eq!(ctx.record_count(), 11);
        let events = drain(&mut rx);
        let inserts = events
            .iter()
            .filter(|e| matches!(e, ChangeEvent::Inserted { .. }))
            .count();
        let deletes = events
            .iter()
            .filter(|e| matches!(e, ChangeEvent::Deleted { .. }))
            .count();
        assert_eq!(inserts, 5);
        assert_eq!(deletes, 4);
    }

    #[test]
    fn batch_patch_script_emits_one_updated_per_row() {
        let (ctx, mut rx) = ctx();
        for _ in 0..4 {
            ctx.seed_one();
        }
        let script = r#"
            for id in ids() {
                patch(id, #{ balance: get(id).balance + rand_float(-5.0, 5.0) });
            }
        "#;
        run_tick(&ctx, script, 0).unwrap();
        let updates = drain(&mut rx)
            .iter()
            .filter(|e| matches!(e, ChangeEvent::Updated { .. }))
            .count();
        assert_eq!(updates, 4);
    }

    #[test]
    fn a_broken_script_reports_not_panics() {
        let (ctx, _rx) = ctx();
        let err = run_tick(&ctx, "no_such_verb()", 0).unwrap_err();
        assert!(
            err.contains("no_such_verb"),
            "error names the problem: {err}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn live_effect_broadcasts_scripted_updates() {
        // The full public path, like the fifo live test: build → subscribe →
        // observe. Real (short) interval; spawn_blocking needs real threads.
        let table = crate::FakerTable::build(
            "scripted",
            vec![
                FakerColumn {
                    name: "id".into(),
                    ty: "string".into(),
                    flags: vec!["id".into()],
                },
                FakerColumn {
                    name: "name".into(),
                    ty: "string".into(),
                    flags: vec![],
                },
            ],
            "id",
            Box::new(RhaiEffect {
                count: 2,
                interval: Duration::from_millis(10),
                script: r#"set(pick(ids()), "name", "ticked")"#.into(),
            }),
        );
        let mut rx = table.events.subscribe();
        let got = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match rx.recv().await {
                    Ok(ChangeEvent::Updated { .. }) => return true,
                    Ok(_) => continue,
                    Err(_) => return false,
                }
            }
        })
        .await
        .expect("expected an Updated within 2s");
        assert!(got);
        drop(table); // aborts the loop
    }
}
