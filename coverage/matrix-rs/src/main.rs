//! Per-test code regions for coverage/matrix.sh.
//!
//! usage: matrix-rs <list.json> <crate_dir> <out_dir> [jobs]
//!
//! Runs every non-ignored test from `cargo nextest list --message-format json`
//! alone with its own LLVM profile, then reads the code regions it covered
//! inside <crate_dir>/src straight from the profile and the test binary's
//! coverage mapping. The mapping is read once per binary rather than once per
//! test, which is what `llvm-cov export` costs. Writes <out_dir>/covered.json
//! for matrix.py:
//!
//!   {"tests": n, "internal": [key], "failed": [[key, code]],
//!    "covered": {key: [[file, ls, cs, le, ce]]}}
//!
//! Region spans are the ones `llvm-cov export` reports, file paths relative to
//! <crate_dir>.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use llvm_profparser::coverage::coverage_mapping::read_object_file;
use llvm_profparser::coverage::{
    Counter, CounterMappingRegion, CounterType, ExprKind, Expression, FunctionRecordV3, RegionKind,
};
use llvm_profparser::{CoverageMappingInfo, parse};
use rayon::prelude::*;
use serde_json::{Value, json};

type Region = (String, usize, usize, usize, usize);

/// Unit tests built into a crate target are internal; `tests/` binaries are blackbox.
const UNIT_KINDS: &[&str] = &[
    "lib",
    "bin",
    "proc-macro",
    "cdylib",
    "staticlib",
    "rlib",
    "dylib",
];

struct Test {
    key: String,
    binary: PathBuf,
    cwd: PathBuf,
    name: String,
    profile: PathBuf,
}

/// One of the crate's functions with its code regions under src/.
struct Local<'a> {
    func: &'a FunctionRecordV3,
    regions: Vec<(&'a CounterMappingRegion, Region)>,
}

fn value(c: &Counter, counts: &[u64], exprs: &[Expression]) -> i64 {
    match c.kind {
        CounterType::Zero => 0,
        CounterType::ProfileInstrumentation => {
            counts.get(c.id as usize).copied().unwrap_or(0) as i64
        }
        CounterType::Expression(_) => {
            let e = &exprs[c.id as usize];
            let (l, r) = (value(&e.lhs, counts, exprs), value(&e.rhs, counts, exprs));
            match e.kind {
                ExprKind::Add => l + r,
                ExprKind::Subtract => l - r,
            }
        }
    }
}

/// The path under the crate's directory, if `path` is one of its sources.
/// Cargo reuses a binary built from another checkout of an identical crate
/// (it fingerprints workspace members by their path in the workspace), so a
/// mapping can name `<elsewhere>/<crate dir>/src/...`: that counts too.
fn relative<'p>(path: &'p Path, crate_dirs: &[PathBuf], name: &str) -> Option<&'p Path> {
    if let Some(rel) = crate_dirs.iter().find_map(|d| path.strip_prefix(d).ok()) {
        return Some(rel);
    }
    let components: Vec<_> = path.components().collect();
    let at = components
        .windows(2)
        .rposition(|w| w[0].as_os_str() == name && w[1].as_os_str() == "src")?;
    path.strip_prefix(components[..=at].iter().collect::<PathBuf>())
        .ok()
}

/// The crate's functions in one binary, with their code regions under src/.
fn local<'a>(info: &'a CoverageMappingInfo, crate_dirs: &[PathBuf], name: &str) -> Vec<Local<'a>> {
    let mut paths_by_ref: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut out = vec![];
    for func in &info.cov_fun {
        let paths = paths_by_ref
            .entry(func.header.filenames_ref)
            .or_insert_with(|| info.get_files_from_id(func.header.filenames_ref));
        let regions: Vec<_> = func
            .regions
            .iter()
            .filter(|r| r.kind == RegionKind::Code)
            .filter_map(|r| {
                let path = paths.get(r.file_id)?;
                let rel = relative(path, crate_dirs, name).filter(|rel| rel.starts_with("src"))?;
                let l = &r.loc;
                Some((
                    r,
                    (
                        rel.to_string_lossy().into_owned(),
                        l.line_start,
                        l.column_start,
                        l.line_end,
                        l.column_end,
                    ),
                ))
            })
            .collect();
        if !regions.is_empty() {
            out.push(Local { func, regions });
        }
    }
    out
}

fn covered(local: &[Local], profile: &Path) -> Result<BTreeSet<Region>> {
    let profile = parse(profile)?;
    let by_hash: HashMap<u64, (u64, &[u64])> = profile
        .records()
        .iter()
        .filter_map(|r| Some((r.name_hash?, (r.hash?, r.record.counts.as_slice()))))
        .collect();
    let mut out = BTreeSet::new();
    for l in local {
        let Some(&(fn_hash, counts)) = by_hash.get(&l.func.header.name_hash) else {
            continue;
        };
        if fn_hash != l.func.header.fn_hash {
            continue;
        }
        for (region, key) in &l.regions {
            if value(&region.count, counts, &l.func.expressions) > 0 {
                out.insert(key.clone());
            }
        }
    }
    Ok(out)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        anyhow::bail!("usage: matrix-rs <list.json> <crate_dir> <out_dir> [jobs]");
    }
    let list: Value =
        serde_json::from_slice(&fs::read(&args[1]).context("reading the test list")?)?;
    let crate_dir = std::path::absolute(&args[2])?;
    let crate_dirs = vec![crate_dir.clone(), fs::canonicalize(&crate_dir)?];
    let crate_name = crate_dir
        .file_name()
        .context("crate_dir has no name")?
        .to_string_lossy()
        .into_owned();
    let out = std::path::absolute(&args[3])?;
    let jobs = args.get(4).map(|j| j.parse()).transpose()?.unwrap_or(8);
    let prof = out.join("prof");
    fs::create_dir_all(&prof)?;

    let mut tests = vec![];
    let mut internal = vec![];
    for suite in list["rust-suites"]
        .as_object()
        .context("no rust-suites")?
        .values()
    {
        let id = suite["binary-id"].as_str().unwrap_or_default();
        for (name, case) in suite["testcases"].as_object().into_iter().flatten() {
            if case["ignored"].as_bool() == Some(true)
                || case["filter-match"]["status"] != "matches"
            {
                continue;
            }
            let key = format!("{id}::{name}");
            if UNIT_KINDS.contains(&suite["kind"].as_str().unwrap_or_default()) {
                internal.push(key.clone());
            }
            tests.push(Test {
                profile: prof.join(format!("{}.profraw", tests.len())),
                key,
                binary: suite["binary-path"].as_str().unwrap_or_default().into(),
                cwd: suite["cwd"].as_str().unwrap_or_default().into(),
                name: name.clone(),
            });
        }
    }

    rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build_global()?;
    let codes: Vec<Option<i32>> = tests
        .par_iter()
        .map(|t| {
            Command::new(&t.binary)
                .args(["--exact", &t.name])
                .current_dir(&t.cwd)
                .env("LLVM_PROFILE_FILE", &t.profile)
                .env("RUST_TEST_THREADS", "1")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .ok()
                .and_then(|s| s.code())
        })
        .collect();

    // Each binary's mapping is read once and serves all of its tests.
    let mut by_binary: BTreeMap<&Path, Vec<usize>> = BTreeMap::new();
    for (i, t) in tests.iter().enumerate() {
        by_binary.entry(&t.binary).or_default().push(i);
    }
    let regions: Vec<(usize, Option<BTreeSet<Region>>)> = by_binary
        .par_iter()
        .flat_map(|(binary, idx)| {
            let first = idx.iter().find_map(|&i| parse(&tests[i].profile).ok());
            let info =
                first.and_then(|p| read_object_file(binary, p.version().unwrap_or_default()).ok());
            let local = info
                .as_ref()
                .map(|info| local(info, &crate_dirs, &crate_name));
            idx.iter()
                .map(|&i| {
                    let set = local
                        .as_ref()
                        .and_then(|l| covered(l, &tests[i].profile).ok());
                    let _ = fs::remove_file(&tests[i].profile);
                    (i, set)
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let mut covered_json = serde_json::Map::new();
    let mut failed = vec![];
    let mut sets: Vec<Option<BTreeSet<Region>>> = vec![None; tests.len()];
    for (i, set) in regions {
        sets[i] = set;
    }
    for (i, t) in tests.iter().enumerate() {
        let code = codes[i].unwrap_or(-1);
        match &sets[i] {
            None => failed.push(json!([t.key, if code == 0 { 1 } else { code }])),
            Some(set) => {
                if code != 0 {
                    failed.push(json!([t.key, code]));
                }
                covered_json.insert(
                    t.key.clone(),
                    json!(
                        set.iter()
                            .map(|r| json!([r.0, r.1, r.2, r.3, r.4]))
                            .collect::<Vec<_>>()
                    ),
                );
            }
        }
    }
    internal.sort();
    fs::write(
        out.join("covered.json"),
        serde_json::to_vec(&json!({
            "tests": tests.len(), "internal": internal, "failed": failed, "covered": covered_json,
        }))?,
    )?;
    Ok(())
}
