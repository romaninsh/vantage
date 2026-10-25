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
ROOT = os.path.dirname(HERE)
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
        from overlap import load
        ns = load(os.path.join(HERE, "report", package), crate, package)
        crate_rel = os.path.relpath(os.path.abspath(crate), ROOT)
        found = violations(ns, new_tests(ns, crate_rel, changed_lines(base, [crate_rel])))
        if found:
            report([describe(package, v) for v in found])
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
