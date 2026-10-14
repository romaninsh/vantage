//! Two-pass (progressive) loading, end-to-end, wired to **real** vantage-cmd
//! list + detail scripts (a shell fixture — no mocked records).
//!
//! These tests assert the things that matter for progressive loading:
//! invocation **count**, invocation **order**, and async correctness. They do
//! NOT simulate slow loads or advance a paused clock (the cucumber suite's
//! flakiness comes from clock-advance); instead they run on a real clock with a
//! tiny viewport debounce and a bounded poll helper.
//!
//! The fixture (`fixtures/two_pass_runs.sh`) logs every invocation to
//! `$RUNS_LOG`, so a test reads that file back to assert the exact sequence of
//! `list`/`detail` calls the machinery issued.

mod hydration;
mod listing;
mod paging;
mod sharing;
mod splice;
mod stamps;
#[path = "../support/mod.rs"]
mod support;

use std::path::Path;
use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_cmd::{Cmd, CmdSpec};
use vantage_diorama::{Cache, Dio, Lens, TableScenery};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

const ENTITY: &str = "gh-workflow-runs";

/// List script: build `["list", offset, limit, <branch?>]` from scope and parse
/// the JSON the fixture emits.
const LIST: &str = r#"
    let args = ["list", offset, limit];
    for c in conditions { args += [c.value]; }
    parse_json(run(args).stdout)
"#;

/// Detail script: run the fixture with `id` in scope.
const DETAIL: &str = r#"parse_json(run(["detail", id]).stdout)"#;

fn fixture() -> String {
    format!(
        "{}/tests/fixtures/two_pass_runs.sh",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// A `Cmd` over the fixture, with the list + detail scripts and the log path
/// baked into the environment.
fn make_cmd(log: &Path) -> Cmd {
    Cmd::new(fixture())
        .with_env("RUNS_LOG", log.to_string_lossy().to_string())
        .with_table(ENTITY, CmdSpec::new(LIST).with_detail(DETAIL))
}

/// A `Vista` over the cmd source: its list script serves windows, and its
/// detail script answers a read by id.
fn master(cmd: &Cmd) -> vantage_vista::Vista {
    let table = Table::<Cmd, EmptyEntity>::new(ENTITY, cmd.clone())
        .with_id_column("id")
        .with_column_of::<String>("branch")
        .with_column_of::<String>("detail");
    cmd.vista_factory().from_table(table).expect("master vista")
}

fn cbor_text(v: &CborValue) -> Option<String> {
    match v {
        CborValue::Text(s) => Some(s.clone()),
        _ => None,
    }
}

/// A no-loader Lens over the redb file in `cache_dir`: the two-pass Dio fills
/// its own cache through its list and detail passes.
fn two_pass_lens(cache_dir: &Path) -> Arc<Lens> {
    support::quick_lens(Cache::redb(cache_dir.join("cache.redb")))
}

/// Read the fixture's invocation log back as lines.
fn log_lines(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|l| l.to_string())
        .collect()
}

fn count_with_prefix(log: &Path, prefix: &str) -> usize {
    log_lines(log)
        .iter()
        .filter(|l| l.starts_with(prefix))
        .count()
}

fn detail_of(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<String> {
    scenery
        .row(idx)
        .and_then(|r| r.record.get("detail").and_then(cbor_text))
}

fn branch_of(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<String> {
    scenery
        .row(idx)
        .and_then(|r| r.record.get("branch").and_then(cbor_text))
}

/// The two-pass Dio: the master's list script is the list pass, and an
/// augmentation merging `detail` from `detail_cmd`'s detail script, one id at
/// a time, is the detail pass. Augmenting is what makes the Dio two-pass.
async fn two_pass_dio(lens: &Arc<Lens>, list_cmd: &Cmd, detail_cmd: &Cmd) -> Dio {
    support::augmented_dio(
        lens,
        master(list_cmd),
        vec![support::fixed_augmentation(master(detail_cmd), &["detail"])],
    )
    .await
}

async fn open_dio(lens: &Arc<Lens>, cmd: &Cmd) -> Dio {
    two_pass_dio(lens, cmd, cmd).await
}
