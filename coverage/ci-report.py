"""The CI summary: one markdown comment on a PR, or main's baseline.

usage: python3 -I coverage/ci-report.py --run <json> [--jobs <json>] --junit <dir>
           --lcov <file> --stats <dir> [--audit <dir>]
           [--baseline main.json] [--update-baseline out.json] > summary.md

--run: crates, measured, versions, published, all, gated (ci-changes' outputs).
--jobs: {job: result} of the jobs the summary waits on.
--junit: one directory per test group holding nextest's junit.xml.
--stats: <crate>.pairs.json (matrix-rs) and <crate>.findings.json (new_tests.py).
--audit: script/ci-audit's output: audit.json, introduced.json and
affected.json. Only advisories reaching a changed crate are listed, plus any
the PR's lockfile brings in.
Only changed crates are reported, each against main's baseline:
{crate: {lines, covered, tests, identical, near}}. The output starts with a
marker so CI finds and edits the same comment on every run.
"""
import argparse
import json
import os
import sys
import xml.etree.ElementTree as ET

MARKER = "<!-- vantage-ci-report -->"
SKILL = "https://github.com/romaninsh/vantage/blob/main/coverage/matrix-rs/skills/matrix-based-test-cleanup/SKILL.md"


def read_json(path, default):
    return json.load(open(path)) if path and os.path.isfile(path) else default


def junit_counts(junit_dir):
    """{crate: [tests, failed]} from every group's junit.xml (suites are crate::binary)."""
    out = {}
    if not junit_dir or not os.path.isdir(junit_dir):
        return out
    for group in sorted(os.listdir(junit_dir)):
        path = os.path.join(junit_dir, group, "junit.xml")
        if not os.path.isfile(path):
            continue
        for suite in ET.parse(path).getroot().iter("testsuite"):
            crate = suite.get("name", "").split("::")[0]
            row = out.setdefault(crate, [0, 0])
            row[0] += int(suite.get("tests", 0)) - int(suite.get("skipped", 0))
            row[1] += int(suite.get("failures", 0)) + int(suite.get("errors", 0))
    return out


def coverage(lcov, measured):
    """{crate: (lines found, lines hit)}. A file can appear once per test
    group: its lines are merged, hit if any group hit them."""
    dirs = {f"/{c['dir']}/src/": c["name"] for c in measured}
    lines, current = {}, None
    if lcov and os.path.isfile(lcov):
        for row in open(lcov):
            if row.startswith("SF:"):
                path = row[3:].strip()
                crate = next((n for d, n in dirs.items() if d in path), None)
                current = lines.setdefault((crate, path), {}) if crate else None
            elif current is not None and row.startswith("DA:"):
                number, count = row[3:].split(",")[:2]
                current[int(number)] = current.get(int(number), False) or int(count) > 0
    totals = {}
    for (crate, _), hits in lines.items():
        found, hit = totals.get(crate, (0, 0))
        totals[crate] = (found + len(hits), hit + sum(hits.values()))
    return totals


def measure(run, junit, lcov, stats):
    """{crate: {lines, covered, tests, failed, identical, near}} for this run's crates."""
    counts = junit_counts(junit)
    cov = coverage(lcov, json.loads(run.get("measured") or "[]"))
    out = {}
    for crate in json.loads(run.get("crates") or "[]"):
        row = {}
        if cov.get(crate, (0, 0))[0]:
            row["lines"], row["covered"] = cov[crate]
        if crate in counts:
            row["tests"], row["failed"] = counts[crate]
        pairs = read_json(os.path.join(stats or "", f"{crate}.pairs.json"), None)
        if pairs:
            row["identical"], row["near"] = pairs["identical"], pairs["near"]
        out[crate] = row
    return out


def change(now, was, fmt="{:+d}"):
    """` (−4)`, or nothing when unchanged or there's nothing to compare with."""
    if was is None or now == was:
        return ""
    return " (" + fmt.format(now - was).replace("-", "−") + ")"


def pct(r):
    return round(100 * r["covered"] / r["lines"], 1) if r and r.get("lines") else None


def version_cell(crate, versions, published, no_changelog=(), changelogs=None):
    """`2.3.1 (+patch)`: the new version and which part moved, by position
    (+MAJOR, +minor, +patch). The version links to the CHANGELOG, and hovering
    it shows the entry."""
    if crate not in published:
        return ""
    if crate not in versions:
        return "💤 not bumped"
    old, new = versions[crate]
    entry = (changelogs or {}).get(crate)
    shown = new
    if entry:
        env = os.environ.get
        url = f"{env('GITHUB_SERVER_URL', 'https://github.com')}/{env('GITHUB_REPOSITORY', '')}/blob/{env('GITHUB_SHA', 'HEAD')}/{entry['path']}"
        shown = f'[{new}]({url} "{entry["text"].replace(chr(34), chr(39))}")'
    note = " 📝 no CHANGELOG entry" if crate in no_changelog else ""
    if not old:
        return f"{shown} (new){note}"
    o, n = old.split("-")[0].split("."), new.split("-")[0].split(".")
    moved = next((i for i in range(3) if (o + ["0"] * 3)[i] != (n + ["0"] * 3)[i]), None)
    kind = {0: "+MAJOR", 1: "+minor", 2: "+patch"}.get(moved, "+pre")
    return f"{shown} ({kind}){note}"


def crate_row(crate, m, was, versions, published, no_changelog=(), changelogs=None):
    was = was or {}
    tests = "—"
    if "tests" in m:
        tests = f"{m['tests']}{change(m['tests'], was.get('tests'))}"
        if m.get("failed"):
            tests += f" 🔥 {m['failed']}"
    covered = "—"
    if "lines" in m:
        now, before = pct(m), pct(was)
        mark = "" if before is None or now == before else ("🌱 " if now > before else "🍂 ")
        covered = f"{mark}{now:.1f}%{change(now, before, '{:+.1f}')}"
    dupes = overlaps = "—"
    if "identical" in m:
        dupes = f"{'💔' if m['identical'] else '❤️'} {m['identical']}{change(m['identical'], was.get('identical'))}"
        overlaps = f"🔍 {m['near']}{change(m['near'], was.get('near'))}"
    version = version_cell(crate, versions, published, no_changelog, changelogs)
    return f"| `{crate}` | {version} | {tests} | {covered} | {dupes} | {overlaps} |"


def findings_section(stats, crates):
    blocking, near = [], []
    for crate in crates:
        for f in read_json(os.path.join(stats or "", f"{crate}.findings.json"), []):
            line = f"`{f['test']}` ↔ `{f['twin']}`" if f["twin"] else f"`{f['test']}`: {f['detail']}"
            (near if f["kind"] == "near" else blocking).append(f"- {crate}: {line}")
    out = []
    if blocking:
        out += ["", f"**💔 New tests that duplicate others ({len(blocking)})**: merge or drop them "
                f"([cleanup skill]({SKILL}))", *blocking]
    if near:
        out += ["", f"<details><summary>🔍 New tests that overlap others ({len(near)})</summary>", "", *near,
                "", "</details>"]
    return out


def audit_section(audit_dir, changed):
    """Advisories reaching a changed crate (named after each), plus any the
    PR's lockfile brings in wherever they land; the brought-in ones first."""
    report = read_json(os.path.join(audit_dir or "", "audit.json"), None)
    if not report:
        return []
    introduced = set(read_json(os.path.join(audit_dir, "introduced.json"), []))
    affected = read_json(os.path.join(audit_dir, "affected.json"), {})
    rows = []
    for v in report.get("vulnerabilities", {}).get("list", []):
        adv, pkg = v["advisory"], v["package"]
        reached = sorted(set(affected.get(adv["id"], [])) & set(changed))
        if not reached and adv["id"] not in introduced:
            continue
        fix = v.get("versions", {}).get("patched") or []
        mark = f"💊 upgrade to `{fix[0]}`" if fix else "☠️ no fixed version"
        via = f" · in {', '.join(f'`{c}`' for c in reached)}" if reached else ""
        line = f"[{adv['id']}]({adv.get('url') or 'https://rustsec.org/advisories/' + adv['id']}) " \
               f"`{pkg['name']} {pkg['version']}`: {adv['title']} · {mark}{via}"
        rows.append((adv["id"] not in introduced, f"- {'**new** ' if adv['id'] in introduced else ''}{line}"))
    if not rows:
        return []
    new = [r for known, r in rows if not known]
    known = [r for k, r in rows if k]
    out = []
    if new:
        out += ["", f"**☠️ This PR's lockfile brings in {len(new)} advisor{'y' if len(new) == 1 else 'ies'}**", *new]
    if known:
        fixable = sum("💊" in r for r in known)
        out += ["", f"<details><summary>💊 {len(known)} known advisor{'y' if len(known) == 1 else 'ies'} "
                f"in the changed crates' dependencies, {fixable} fixed by an upgrade</summary>",
                "", *known, "", "</details>"]
    return out


def headline(run, rows, jobs):
    crates = json.loads(run.get("crates") or "[]")
    tests = sum(m.get("tests", 0) for m in rows.values())
    failed_jobs = sorted(j for j, r in jobs.items() if r in ("failure", "cancelled"))
    status = f"🔥 {', '.join(f'`{j}`' for j in failed_jobs)} failed" if failed_jobs else "✨ all green"
    scope = "every crate" if run.get("all") == "true" else (
        ", ".join(f"`{c}`" for c in crates) if 0 < len(crates) <= 3 else f"{len(crates)} changed crates")
    return [f"### {status} · {tests:,} tests · {scope}"]


def main():
    ap = argparse.ArgumentParser()
    for flag in ("--run", "--jobs", "--junit", "--lcov", "--stats", "--audit", "--baseline", "--update-baseline"):
        ap.add_argument(flag, default="")
    a = ap.parse_args()
    run = json.loads(a.run or "{}")
    jobs = json.loads(a.jobs or "{}")
    baseline = read_json(a.baseline, {})
    rows = measure(run, a.junit, a.lcov, a.stats)
    if a.update_baseline:
        updated = {c: v for c, v in baseline.items() if isinstance(v, dict)}
        updated.update({c: {k: v for k, v in m.items() if k != "failed"} for c, m in rows.items() if m})
        json.dump(updated, open(a.update_baseline, "w"), indent=1, sort_keys=True)

    versions = json.loads(run.get("versions") or "{}")
    published = set(json.loads(run.get("published") or "[]"))
    no_changelog = set(json.loads(run.get("no_changelog") or "[]"))
    changelogs = json.loads(run.get("changelogs") or "{}")
    out = [MARKER] + headline(run, rows, jobs)
    shown = {c: m for c, m in rows.items() if m or c in published}
    if shown:
        out += ["", "| crate | version | tests | covered | dupes | overlaps |", "|---|---|--:|--:|--:|--:|"]
        for crate in sorted(shown):
            was = baseline.get(crate) if isinstance(baseline.get(crate), dict) else None
            out.append(crate_row(crate, shown[crate], was, versions, published, no_changelog, changelogs))
    out += findings_section(a.stats, sorted(rows))
    out += audit_section(a.audit, rows)
    env = os.environ.get
    url = f"{env('GITHUB_SERVER_URL', '')}/{env('GITHUB_REPOSITORY', '')}/actions/runs/{env('GITHUB_RUN_ID', '')}"
    if run.get("gated") != "true":
        out += ["", "🧊 Gated suites didn't run. Add the `ALL_TESTS` label to test every crate."]
    out += ["", f"<sub>{env('GITHUB_SHA', '')[:8]} · [run]({url}) · compared with main's last baseline</sub>"]
    print("\n".join(out))


if __name__ == "__main__":
    sys.exit(main())
