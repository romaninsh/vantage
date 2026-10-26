//! Per-test code regions for coverage/matrix.sh.
//!
//! usage: matrix-rs <list.json> <crate_dir> <out_dir> [jobs]
//!        matrix-rs group <list.json> <out_root> [jobs]
//!
//! Runs every non-ignored test from `cargo nextest list --message-format json`
//! alone with its own LLVM profile, then reads the code regions it covered
//! inside its crate's src/ straight from the profile and the test binary's
//! coverage mapping. The mapping is read once per binary rather than once per
//! test, which is what `llvm-cov export` costs. `group` takes a list of
//! several crates and runs all their tests in one pool (each crate's dir is
//! its suites' `cwd`), so no core waits for one crate's last tests; it writes
//! each crate's files to <out_root>/<package>/, with the excluded paths from
//! MATRIX_EXCLUDE_JSON ({package: [glob]}). Writes <out_dir>/covered.json for
//! matrix.py:
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
    krate: usize,
}

/// A crate whose tests are measured: regions count under its dir's src/.
struct Crate {
    dirs: Vec<PathBuf>,
    name: String,
    out: PathBuf,
    excluded: Vec<String>,
}

impl Crate {
    fn new(dir: &Path, out: PathBuf, excluded: Vec<String>) -> Result<Self> {
        let dir = std::path::absolute(dir)?;
        let name = dir
            .file_name()
            .context("crate dir has no name")?
            .to_string_lossy()
            .into_owned();
        let canonical = fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        Ok(Crate { dirs: vec![dir, canonical], name, out, excluded })
    }
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
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let group = args.first().map(String::as_str) == Some("group");
    if group {
        args.remove(0);
    }
    if args.len() < if group { 2 } else { 3 } {
        anyhow::bail!(
            "usage: matrix-rs <list.json> <crate_dir> <out_dir> [jobs]\n       matrix-rs group <list.json> <out_root> [jobs]"
        );
    }
    let list: Value =
        serde_json::from_slice(&fs::read(&args[0]).context("reading the test list")?)?;
    let excluded_of = |var: &str| -> Vec<String> {
        var.split(',').filter(|g| !g.is_empty()).map(str::to_string).collect()
    };
    // group: one crate per package in the list; otherwise the one crate given.
    let (root, jobs_arg) = if group { (&args[1], args.get(2)) } else { (&args[2], args.get(3)) };
    let root = std::path::absolute(root)?;
    let jobs = jobs_arg.map(|j| j.parse()).transpose()?.unwrap_or(8);
    let group_excluded: HashMap<String, Vec<String>> = std::env::var("MATRIX_EXCLUDE_JSON")
        .ok()
        .map(|j| serde_json::from_str(&j))
        .transpose()?
        .unwrap_or_default();
    let mut crates: Vec<Crate> = vec![];
    let mut crate_of: HashMap<String, usize> = HashMap::new();
    if !group {
        let excluded = excluded_of(&std::env::var("MATRIX_EXCLUDE").unwrap_or_default());
        crates.push(Crate::new(Path::new(&args[1]), root.clone(), excluded)?);
    }
    let prof = root.join("prof");
    fs::create_dir_all(&prof)?;

    let mut tests = vec![];
    let mut internal = vec![];
    for suite in list["rust-suites"]
        .as_object()
        .context("no rust-suites")?
        .values()
    {
        let id = suite["binary-id"].as_str().unwrap_or_default();
        let krate = if group {
            let package = suite["package-name"].as_str().unwrap_or_default().to_string();
            match crate_of.get(&package) {
                Some(&k) => k,
                None => {
                    let dir = Path::new(suite["cwd"].as_str().unwrap_or_default());
                    let excluded = group_excluded.get(&package).cloned().unwrap_or_default();
                    crates.push(Crate::new(dir, root.join(&package), excluded)?);
                    crate_of.insert(package, crates.len() - 1);
                    crates.len() - 1
                }
            }
        } else {
            0
        };
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
                krate,
            });
        }
    }

    rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build_global()?;
    let run = |t: &Test| {
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
    };

    // Per binary: run one test and read the binary's mapping with its profile
    // (once), then run the rest, each read and deleted as soon as it ends. A
    // profile holds every counter of its binary, so keeping a whole binary's
    // profiles until its last test ends can fill the disk.
    let started = std::time::Instant::now();
    let mut by_binary: BTreeMap<&Path, Vec<usize>> = BTreeMap::new();
    for (i, t) in tests.iter().enumerate() {
        by_binary.entry(&t.binary).or_default().push(i);
    }
    let results: Vec<(usize, Option<i32>, Option<BTreeSet<Region>>)> = by_binary
        .par_iter()
        .flat_map(|(binary, idx)| {
            let mapping = |i: usize| {
                let p = parse(&tests[i].profile).ok()?;
                read_object_file(binary, p.version().unwrap_or_default()).ok()
            };
            let measure = |l: &Option<Vec<Local<'_>>>, i: usize| {
                let set = l.as_ref().and_then(|l| covered(l, &tests[i].profile).ok());
                let _ = fs::remove_file(&tests[i].profile);
                set
            };
            let first = idx[0];
            let first_code = run(&tests[first]);
            let mut codes = vec![first_code];
            let mut info = mapping(first);
            if info.is_none() {
                // The first test left no profile (it crashed before exiting):
                // run the rest, then take the mapping from any profile there is.
                codes.extend(idx[1..].par_iter().map(|&i| run(&tests[i])).collect::<Vec<_>>());
                info = idx.iter().find_map(|&i| mapping(i));
            }
            let k = &crates[tests[first].krate];
            let found = info.as_ref().map(|info| local(info, &k.dirs, &k.name));
            if codes.len() == idx.len() {
                return idx
                    .par_iter()
                    .zip(codes)
                    .map(|(&i, code)| (i, code, measure(&found, i)))
                    .collect::<Vec<_>>();
            }
            let mut out = vec![(first, first_code, measure(&found, first))];
            out.par_extend(idx[1..].par_iter().map(|&i| {
                let code = run(&tests[i]);
                (i, code, measure(&found, i))
            }));
            out
        })
        .collect();
    eprintln!(
        "matrix-rs: {} tests in {} binaries, {jobs} at a time, in {:.1?}",
        tests.len(),
        by_binary.len(),
        started.elapsed()
    );
    let mut codes = vec![None; tests.len()];
    let mut regions = Vec::with_capacity(tests.len());
    for (i, code, set) in results {
        codes[i] = code;
        regions.push((i, set));
    }

    let mut sets: Vec<Option<BTreeSet<Region>>> = vec![None; tests.len()];
    for (i, set) in regions {
        sets[i] = set;
    }
    internal.sort();
    for (k, krate) in crates.iter().enumerate() {
        let mine: Vec<usize> = (0..tests.len()).filter(|&i| tests[i].krate == k).collect();
        let mut covered_json = serde_json::Map::new();
        let mut failed = vec![];
        let mut own_internal = vec![];
        for &i in &mine {
            let t = &tests[i];
            let code = codes[i].unwrap_or(-1);
            if internal.binary_search(&t.key).is_ok() {
                own_internal.push(t.key.clone());
            }
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
        fs::create_dir_all(&krate.out)?;
        fs::write(
            krate.out.join("covered.json"),
            serde_json::to_vec(&json!({
                "tests": mine.len(), "internal": own_internal, "failed": failed, "covered": covered_json,
            }))?,
        )?;
        let blackbox: Vec<&BTreeSet<Region>> = mine
            .iter()
            .filter(|&&i| internal.binary_search(&tests[i].key).is_err())
            .filter_map(|&i| sets[i].as_ref())
            .collect();
        let summary = pairs(&blackbox, &krate.excluded);
        fs::write(krate.out.join("pairs.json"), serde_json::to_vec(&summary)?)?;
    }
    Ok(())
}

/// A region path under an excluded glob: `dir/**` covers everything under
/// `dir/`, anything else must match exactly (the forms coverage/config.yaml uses).
fn is_excluded(path: &str, globs: &[String]) -> bool {
    globs.iter().any(|g| match g.strip_suffix("/**") {
        Some(dir) => path.starts_with(&format!("{dir}/")),
        None => path == g,
    })
}

/// Duplicate measures for the CI report, over blackbox tests with at least
/// FLOOR regions: `identical` counts tests with an exact twin (a group of k
/// counts k - 1, the ones that could go), `near` counts distinct pairs that
/// share at least NEAR of their regions without being identical.
fn pairs(sets: &[&BTreeSet<Region>], excluded: &[String]) -> Value {
    const FLOOR: usize = 20;
    const NEAR: f64 = 0.98;
    let mut ids: HashMap<&Region, u32> = HashMap::new();
    let mut tests: Vec<Vec<u32>> = sets
        .iter()
        .map(|set| {
            let mut v: Vec<u32> = set
                .iter()
                .filter(|r| !is_excluded(&r.0, excluded))
                .map(|r| {
                    let next = ids.len() as u32;
                    *ids.entry(r).or_insert(next)
                })
                .collect();
            v.sort_unstable();
            v
        })
        .filter(|v| v.len() >= FLOOR)
        .collect();
    tests.sort();
    let mut identical = 0;
    for group in tests.chunk_by(|a, b| a == b) {
        identical += group.len() - 1;
    }
    tests.dedup();
    let shared = |a: &[u32], b: &[u32]| {
        let (mut i, mut j, mut n) = (0, 0, 0);
        while i < a.len() && j < b.len() {
            match a[i].cmp(&b[j]) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => (n, i, j) = (n + 1, i + 1, j + 1),
            }
        }
        n
    };
    let near: usize = (0..tests.len())
        .into_par_iter()
        .map(|i| {
            (i + 1..tests.len())
                .filter(|&j| {
                    let (a, b) = (&tests[i], &tests[j]);
                    let (small, big) = (a.len().min(b.len()), a.len().max(b.len()));
                    // Jaccard can't reach NEAR when the sizes differ this much.
                    if (small as f64) < NEAR * big as f64 {
                        return false;
                    }
                    let n = shared(a, b);
                    n as f64 / (a.len() + b.len() - n) as f64 >= NEAR
                })
                .count()
        })
        .sum();
    json!({"tests": sets.len(), "identical": identical, "near": near})
}
