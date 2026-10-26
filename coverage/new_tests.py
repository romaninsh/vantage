"""Fail new tests that duplicate others, judged against the coverage matrix.

usage: python3 -I new_tests.py clusters <package> <crate_dir> <base>
       python3 -I new_tests.py internal <base> <crate_dir>...

A test is new when its line range overlaps a line the branch added or changed
since <base>. <base> may be a `<base>...<head>` range to judge another commit.

`clusters` reads coverage/report/<package>/ (run coverage/matrix.sh first).
A new test that covers at least FLOOR regions, exactly the same set as another
test, fails unless it has an approved exception. A new test sharing at least
NEAR of its regions with another is reported as a possible duplicate and
doesn't fail. Overlap among old tests alone is cleanup work, not this branch's.
Both point at the matrix-based-test-cleanup skill.

`internal` fails any `#[cfg(test)]` line or `src/**/tests*` file added under a
crate's src/. Both exit 1 on a violation.
"""
import os
import re
import subprocess
import sys
from collections import namedtuple

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
# The workspace is where this runs, not where the script lives (.ci/ in another repo).
ROOT = subprocess.run(["git", "rev-parse", "--show-toplevel"], check=True, capture_output=True,
                      text=True).stdout.strip()
sys.path.insert(0, HERE)

Violation = namedtuple("Violation", "kind test twin detail")
FLOOR, NEAR = 20, 0.98
SKILL = "coverage/matrix-rs/skills/matrix-based-test-cleanup/SKILL.md"
HUNK = re.compile(r"^@@ -\S+ \+(\d+)(?:,(\d+))? @@")


def git_diff(base, paths, *flags):
    # A `<base>...<head>` range judges a commit other than HEAD.
    span = base if "..." in base else f"{base}...HEAD"
    return subprocess.run(
        ["git", "diff", "--no-color", "-U0", *flags, span, "--", *paths],
        cwd=ROOT, check=True, capture_output=True, text=True,
    ).stdout


def parse_hunks(text):
    """{repo-relative path: head line numbers added or changed}."""
    out, path = {}, None
    for line in text.splitlines():
        if line.startswith("+++ "):
            path = None if line[4:] == "/dev/null" else line[6:]
            if path is not None:
                out.setdefault(path, set())
        elif path is not None and (m := HUNK.match(line)):
            first, count = int(m[1]), int(m[2] or 1)
            out[path].update(range(first, first + count))
    return out


def changed_lines(base, paths):
    return parse_hunks(git_diff(base, paths))


FN = re.compile(r"^\s*(pub(\(crate\))?\s+)?(async\s+)?fn\s+([A-Za-z0-9_]+)\s*\(")


def fn_ranges(lines):
    """(name, first, last) of every fn in a file, 1-based, attributes and doc
    comments included (the same span overlap.locate gives a test)."""
    for i, line in enumerate(lines):
        if not (m := FN.match(line)):
            continue
        top = i
        while top > 0 and lines[top - 1].strip().startswith(("#[", "///", "//")):
            top -= 1
        depth, end, opened = 0, i, False
        for j in range(i, len(lines)):
            depth += lines[j].count("{") - lines[j].count("}")
            opened = opened or "{" in lines[j]
            if opened and depth <= 0:
                end = j
                break
        yield m[4], top + 1, end + 1


def light(package, crate):
    """What the CI check needs from a matrix run, without overlap.load's
    clustering and per-test source search: blackbox region sets, exceptions,
    and a twin's location looked up only when one is reported."""
    import json

    import config
    import exceptions
    out = os.path.join(ROOT, "coverage", "report", package)
    data = json.load(open(os.path.join(out, "regions.json")))
    left_out = config.excluded(package)
    dropped = {i for i, r in enumerate(data["regions"]) if left_out(r[0])}
    internal = set(data.get("internal", []))
    sets = {k: set(v) - dropped for k, v in data["tests"].items() if k not in internal}
    accepted, rejected = exceptions.check(package)

    def short(key):
        return key.split("::", 1)[1]

    def where(key):
        name = key.rsplit("::", 1)[-1]
        for root, _, files in os.walk(os.path.join(crate, "tests")):
            for f in sorted(files):
                if f.endswith(".rs"):
                    path = os.path.join(root, f)
                    for n, first, last in fn_ranges(open(path, encoding="utf-8").read().splitlines()):
                        if n == name:
                            return f"{os.path.relpath(path, crate)}:{first}-{last}"
        return "?"

    from types import SimpleNamespace
    return SimpleNamespace(sets=sets, exempt={k: accepted[short(k)] for k in sets if short(k) in accepted},
                           rejected=rejected, short=short, where=where)


def changed_tests(ns, crate_rel, changed):
    """Tests whose function overlaps a changed line, found by scanning only the
    changed files. A key matches a changed fn of its name in a file its path
    points at; a name only one test has needs no path."""
    fns = []
    for path, lines in changed.items():
        if lines and path.endswith(".rs") and path.startswith(crate_rel.rstrip("/") + "/"):
            text = open(os.path.join(ROOT, path), encoding="utf-8").read().splitlines()
            rel = os.path.relpath(path, crate_rel)
            fns += [(rel, n) for n, first, last in fn_ranges(text) if any(first <= x <= last for x in lines)]
    by_name = {}
    for key in ns.sets:
        by_name.setdefault(key.rsplit("::", 1)[-1], []).append(key)
    found = set()
    for rel, name in fns:
        parts = re.split(r"[/.]", rel)
        for key in by_name.get(name, []):
            binary, path = key.split("::", 1)
            score = sum(1 for s in path.split("::")[:-1] if s in parts) + (binary.split("::")[-1] in parts)
            if score or len(by_name[name]) == 1:
                found.add(key)
    return found


def new_tests(ns, crate_rel, changed):
    found = set()
    for key, (rel, first, last, *_) in ns.loc.items():
        if rel == "?" or key not in ns.sets:
            continue
        lines = changed.get(os.path.join(crate_rel, rel), ())
        if any(first <= n <= last for n in lines):
            found.add(key)
    return found


def violations(ns, new):
    """Blocking: a new test with exactly another test's regions (at least FLOOR),
    and rejected exception tokens. Advisory: a new test sharing at least NEAR."""
    out = []
    for n in sorted(new):
        sn = ns.sets.get(n)
        if n in ns.exempt or not sn or len(sn) < FLOOR:
            continue
        twin, near = None, None
        for o, so in sorted(ns.sets.items()):
            if o == n or not so:
                continue
            if so == sn:
                twin = o
                break
            shared = len(sn & so) / len(sn | so)
            if shared >= NEAR and near is None:
                near = (o, shared)
        if twin:
            out.append(Violation("identical", ns.short(n), ns.short(twin), ns.where(twin)))
        elif near:
            out.append(Violation("near", ns.short(n), ns.short(near[0]), f"{near[1]:.0%} shared, {ns.where(near[0])}"))
    for entry, problem in ns.rejected:
        out.append(Violation("rejected-exception", str(entry.get("test")), "", problem))
    return out


def describe(package, v):
    if v.kind == "rejected-exception":
        return f"{package}: exception for `{v.test}` is rejected: {v.detail}"
    if v.kind == "near":
        return f"{package}: possible duplicate: `{v.test}` and `{v.twin}` ({v.detail})"
    return (
        f"{package}: new test `{v.test}` covers exactly what `{v.twin}` does ({v.detail}).\n"
        f"  Merge its cases into that test, or drop it ({SKILL}).\n"
        f"  If it is a genuinely different scenario, ask a human to run:\n"
        f"  coverage/vend-exception.sh {package} {v.test} \"<reason>\""
    )


def internal_from_diff(text, crate_dirs):
    found, path, new_file = [], None, False
    for line in text.splitlines():
        if line.startswith("diff --git"):
            path, new_file = None, False
        elif line.startswith("new file mode"):
            new_file = True
        elif line.startswith("+++ "):
            path = None if line[4:] == "/dev/null" else line[6:]
            src = path and any(path.startswith(f"{d.rstrip('/')}/src/") for d in crate_dirs)
            if not src:
                path = None
            elif new_file and re.search(r"/tests[^/]*$|/tests/", path[path.index("/src/"):]):
                found.append(f"{path}:1")
                path = None
        elif path is not None and (m := HUNK.match(line)):
            n = int(m[1])
        elif path is not None and line.startswith("+"):
            if "#[cfg(test)]" in line:
                found.append(f"{path}:{n}")
            n += 1
    return sorted(found)


def report(lines):
    text = "\n\n".join(lines)
    print(text)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a") as f:
            f.write("### Duplicate tests\n\n```\n" + text + "\n```\n")


def main(argv):
    if len(argv) == 4 and argv[0] == "clusters":
        package, crate, base = argv[1:]
        ns = light(package, os.path.abspath(crate))
        crate_rel = os.path.relpath(os.path.abspath(crate), ROOT)
        found = violations(ns, changed_tests(ns, crate_rel, changed_lines(base, [crate_rel])))
        if found:
            report([describe(package, v) for v in found])
        # The same findings as data, for the CI report.
        import json
        with open(os.path.join(ROOT, "coverage", "report", package, "findings.json"), "w") as f:
            json.dump([v._asdict() for v in found], f)
        blocking = [v for v in found if v.kind != "near"]
        if not blocking:
            print(f"{package}: no new test duplicates another")
        return 1 if blocking else 0
    if len(argv) >= 3 and argv[0] == "internal":
        base, dirs = argv[1], [os.path.relpath(os.path.abspath(d), ROOT) for d in argv[2:]]
        found = internal_from_diff(git_diff(base, [f"{d}/src" for d in dirs]), dirs)
        if found:
            report([f"new internal test (tests go under tests/ and use the public API): {f}" for f in found])
            return 1
        return 0
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
