"""Fail new tests that duplicate others, judged against the coverage matrix.

usage: python3 -I new_tests.py clusters <package> <crate_dir> <base>
       python3 -I new_tests.py internal <base> <crate_dir>...

A test is new when its line range overlaps a line the branch added or changed
since <base>. <base> may be a `<base>...<head>` range to judge another commit. `clusters` reads coverage/report/<package>/ (run
coverage/matrix.sh first) and fails a cluster that holds a new test with any
other test, unless the new test has an approved exception. Clusters of old
tests alone pass: existing overlap is cleanup work, not this branch's.
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

Violation = namedtuple("Violation", "kind test cluster anchor number")
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
    out = []
    for number, cluster in enumerate(ns.clusters, 1):
        judged = [k for k in cluster if k in new and k not in ns.exempt]
        if not judged:
            continue
        names = [ns.short(k) for k in cluster]
        anchor = ns.short(cluster[0])
        old = [k for k in cluster if k not in new]
        if old:
            out += [Violation("joins-existing", ns.short(k), names, anchor, number) for k in judged]
        else:
            out.append(Violation("new-with-new", ns.short(judged[0]), names, anchor, number))
    for entry, problem in ns.rejected:
        out.append(Violation("rejected-exception", str(entry.get("test")), [problem], "", 0))
    return out


def describe(package, v):
    if v.kind == "rejected-exception":
        return f"{package}: exception for `{v.test}` is rejected: {v.cluster[0]}"
    what = "joins" if v.kind == "joins-existing" else "shares with other new tests"
    return (
        f"{package}: new test `{v.test}`\n"
        f"  {what} cluster {v.number} (anchor `{v.anchor}`, {len(v.cluster)} tests)\n"
        f"  delete the test it duplicates, or, if it asserts a distinct outcome, ask a human to run:\n"
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
            return 1
        print(f"{package}: no new test duplicates another")
        return 0
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
