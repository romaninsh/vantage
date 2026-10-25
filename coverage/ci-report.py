"""The PR's test and coverage report, as one markdown comment.

usage: python3 -I coverage/ci-report.py <junit dir> <lcov.info> <measured json>
           [--baseline main.json] [--update-baseline out.json] > report.md

<junit dir> holds one directory per test group with nextest's junit.xml.
Starts with a marker line so CI can find and update the same comment on every
run instead of adding one.

--baseline adds a column against main's numbers, {crate: [lines, covered]}.
--update-baseline writes that file: the baseline given, with this run's crates
replaced (post-merge keeps it current crate by crate).
"""
import json
import os
import sys
import xml.etree.ElementTree as ET

MARKER = "<!-- vantage-ci-report -->"
MAX_FAILURES = 20


def groups(junit_dir):
    for name in sorted(os.listdir(junit_dir)):
        path = os.path.join(junit_dir, name, "junit.xml")
        if os.path.isfile(path):
            yield name.removeprefix("junit-"), ET.parse(path).getroot()


def coverage(lcov, measured):
    """{crate: (lines found, lines hit)} for the measured crates. A file can
    appear once per test group: its lines are merged, hit if any group hit them."""
    dirs = {f"/{c['dir']}/src/": c["name"] for c in measured}
    lines = {}  # (crate, file) -> {line: hit}
    current = None
    for row in open(lcov):
        if row.startswith("SF:"):
            path = row[3:].strip()
            crate = next((n for d, n in dirs.items() if d in path), None)
            current = lines.setdefault((crate, path), {}) if crate else None
        elif current is not None and row.startswith("DA:"):
            number, count = row[3:].split(",")[:2]
            current[int(number)] = current.get(int(number), False) or int(count) > 0
    totals = {c["name"]: [0, 0] for c in measured}
    for (crate, _), hits in lines.items():
        totals[crate][0] += len(hits)
        totals[crate][1] += sum(hits.values())
    return totals


def main(junit_dir, lcov, measured_json, baseline_in=None, baseline_out=None):
    measured = json.loads(measured_json)
    baseline = json.load(open(baseline_in)) if baseline_in and os.path.isfile(baseline_in) else {}
    out = [MARKER, "## CI report", "", "| group | passed | failed | skipped | time |", "|---|---:|---:|---:|---:|"]
    failures = []
    for name, root in groups(junit_dir):
        tests, failed, skipped = (int(root.get(k, 0)) for k in ("tests", "failures", "skipped"))
        failed += int(root.get("errors", 0))
        out.append(f"| {name} | {tests - failed - skipped} | {failed} | {skipped} | {float(root.get('time', 0)):.0f}s |")
        for case in root.iter("testcase"):
            problem = case.find("failure")
            if problem is None:
                problem = case.find("error")
            if problem is not None:
                message = (problem.get("message") or problem.text or "").strip().splitlines()
                failures.append(f"- `{case.get('classname')}::{case.get('name')}`"
                                + (f": {message[0][:160]}" if message else ""))
    if failures:
        out += ["", f"**Failed ({len(failures)})**", ""] + failures[:MAX_FAILURES]
        if len(failures) > MAX_FAILURES:
            out.append(f"- … and {len(failures) - MAX_FAILURES} more (see the suite logs)")
    current = coverage(lcov, measured) if lcov and os.path.isfile(lcov) and measured else {}
    if any(found for found, _ in current.values()):
        out += ["", "| crate | lines | covered | vs main |", "|---|---:|---:|---:|"]
        for crate, (found, hit) in sorted(current.items()):
            if not found:
                continue
            pct = 100 * hit / found
            was = baseline.get(crate)
            delta = f"{pct - 100 * was[1] / was[0]:+.1f}" if was and was[0] else "new"
            out.append(f"| {crate} | {found} | {pct:.1f}% | {delta} |")
    if baseline_out:
        updated = dict(baseline)
        updated.update({c: v for c, v in current.items() if v[0]})
        json.dump(updated, open(baseline_out, "w"), indent=1, sort_keys=True)
    run = f"{os.environ.get('GITHUB_SERVER_URL', '')}/{os.environ.get('GITHUB_REPOSITORY', '')}/actions/runs/{os.environ.get('GITHUB_RUN_ID', '')}"
    out += ["", f"<sub>{os.environ.get('GITHUB_SHA', '')[:8]} · [run]({run})</sub>"]
    print("\n".join(out))


if __name__ == "__main__":
    args, options = sys.argv[1:4], dict(zip(sys.argv[4::2], sys.argv[5::2]))
    main(*args, baseline_in=options.get("--baseline"), baseline_out=options.get("--update-baseline"))
