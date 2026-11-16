//! A host over plain maps. `do` and `before` are `;`-separated commands: `insert <table> <id>`
//! and `fail <message>`; any other command is a harness error. Asserts: `count <table> == <n>`,
//! `has <table> <id>`, `no <table> <id>`; anything else raises. A file whose fixtures include
//! `broken` fails to load, and a state holding a `nofork` table fails to fork.

use std::sync::Mutex;

use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::{Value, json};
use vantage_core::{Result, error};
use vantage_tester::{
    ClockOffset, Do, ResolvedStep, Scenario, ScenarioFile, ScenarioHost, StepFailure, StepRun,
    VarDef, Write, WriteOp,
};

#[derive(Clone, Default)]
pub struct MapState {
    pub tables: IndexMap<String, IndexMap<String, Value>>,
    pub minutes: u64,
}

#[derive(Default)]
pub struct MapHost {
    pub runs: Mutex<Vec<String>>,
    pub seeds: Mutex<Vec<u64>>,
}

impl MapHost {
    pub fn runs_of(&self, step: &str) -> usize {
        self.runs
            .lock()
            .unwrap()
            .iter()
            .filter(|s| *s == step)
            .count()
    }
}

fn exec(state: &mut MapState, commands: &str) -> std::result::Result<Vec<Write>, StepFailure> {
    let mut writes = Vec::new();
    for command in commands.split(';').map(str::trim).filter(|c| !c.is_empty()) {
        let words: Vec<&str> = command.split_whitespace().collect();
        match words.as_slice() {
            ["insert", table, id] => {
                state
                    .tables
                    .entry(table.to_string())
                    .or_default()
                    .insert(id.to_string(), json!({}));
                writes.push(Write {
                    table: table.to_string(),
                    op: WriteOp::Insert,
                    id: id.to_string(),
                    columns: IndexMap::new(),
                });
            }
            ["fail", ..] => {
                return Err(StepFailure::new(
                    command.trim_start_matches("fail ").to_string(),
                ));
            }
            _ => return Err(StepFailure::harness(format!("unknown command `{command}`"))),
        }
    }
    Ok(writes)
}

#[async_trait]
impl ScenarioHost for MapHost {
    type State = MapState;

    async fn load(&self, file: &ScenarioFile) -> Result<MapState> {
        if file.fixtures.iter().any(|f| f == "broken") {
            return Err(error!("fixture `broken` can't be read"));
        }
        let mut state = MapState::default();
        exec(&mut state, "insert order o0").map_err(|f| error!(f.message))?;
        Ok(state)
    }

    async fn root(&self, base: &MapState, _scenario: &Scenario, seed: u64) -> Result<MapState> {
        self.seeds.lock().unwrap().push(seed);
        self.fork(base).await
    }

    async fn fork(&self, s: &MapState) -> Result<MapState> {
        if s.tables.contains_key("nofork") {
            return Err(error!("this state can't be forked"));
        }
        Ok(s.clone())
    }

    async fn advance(&self, s: &mut MapState, by: &ClockOffset) -> Result<()> {
        s.minutes += u64::from(by.amount);
        Ok(())
    }

    async fn before(&self, s: &mut MapState, rhai: &str) -> Result<()> {
        exec(s, rhai).map(|_| ()).map_err(|f| error!(f.message))
    }

    async fn run(
        &self,
        s: &mut MapState,
        step: &ResolvedStep,
    ) -> std::result::Result<StepRun, StepFailure> {
        self.runs.lock().unwrap().push(step.step.clone());
        let Some(Do::Script(commands)) = &step.do_ else {
            return Err(StepFailure::new("the map host runs scripts only"));
        };
        let writes = exec(s, commands)?;
        Ok(StepRun {
            result: json!(writes.len()),
            writes,
        })
    }

    async fn assert(&self, s: &MapState, rhai: &str, _result: &StepRun) -> Result<bool> {
        let has = |table: &str, id: &str| s.tables.get(table).is_some_and(|t| t.contains_key(id));
        match rhai.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["count", table, "==", n] => {
                Ok(s.tables.get(*table).map_or(0, |t| t.len()).to_string() == *n)
            }
            ["has", table, id] => Ok(has(table, id)),
            ["no", table, id] => Ok(!has(table, id)),
            _ => Err(error!(format!("can't evaluate `{rhai}`"))),
        }
    }

    async fn declare(
        &self,
        _s: &mut MapState,
        _vars: &IndexMap<String, VarDef>,
        _result: &StepRun,
    ) -> Result<()> {
        Ok(())
    }
}
