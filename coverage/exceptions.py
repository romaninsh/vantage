"""Approved test exceptions: tests the matrix never offers for deletion.

usage: python3 -I exceptions.py vend <package> <test> <reason>
       python3 -I exceptions.py check [package]

Entries live in coverage/exceptions/<package>.yaml, each with three fields:
test, reason and token. The token is an 8-hex nonce and a 12-hex signature of
key|nonce|package|test|reason; the key sits outside the repo in
~/.config/vantage. An entry counts only when its signature checks, its token
is used nowhere else, and its test isn't listed twice.
"""
import hashlib
import os
import secrets
import subprocess
import sys

try:
    import yaml
except ImportError:
    sys.exit("coverage/exceptions.py needs PyYAML (pip install pyyaml)")

DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "exceptions")
KEY = os.path.expanduser("~/.config/vantage/coverage-exceptions.key")
HEADER = """\
# @llm-protected: approved test exceptions. Add entries only with coverage/vend-exception.sh.
#
# Each entry keeps one test out of the matrix's deletion candidates. The token signs the
# package (this file's name), the test and the reason: edit either and the entry stops counting.
"""


def key(create=False):
    # CI has no ~/.config: it passes the same key as a repository secret,
    # stripped like the file, so a secret saved with a newline still signs.
    if os.environ.get("COVERAGE_EXCEPTIONS_KEY", "").strip():
        return os.environ["COVERAGE_EXCEPTIONS_KEY"].strip()
    if not os.path.exists(KEY):
        if not create:
            return None
        os.makedirs(os.path.dirname(KEY), exist_ok=True)
        with open(KEY, "w") as f:
            f.write(secrets.token_hex(32))
        os.chmod(KEY, 0o600)
    return open(KEY).read().strip()


def sign(secret, nonce, package, test, reason):
    payload = "|".join((secret, nonce, package, test, reason))
    return hashlib.sha256(payload.encode()).hexdigest()[:12]


def path(package):
    return os.path.join(DIR, f"{package}.yaml")


def entries(package):
    if not os.path.exists(path(package)):
        return []
    return yaml.safe_load(open(path(package))) or []


def base_entries(base, package):
    """The package's entries as committed at `base`, or [] if it had none."""
    shown = subprocess.run(
        ["git", "show", f"{base}:coverage/exceptions/{package}.yaml"],
        cwd=os.path.dirname(DIR), capture_output=True, text=True,
    )
    return (yaml.safe_load(shown.stdout) or []) if shown.returncode == 0 else []


def packages():
    if not os.path.isdir(DIR):
        return []
    return sorted(f[:-5] for f in os.listdir(DIR) if f.endswith(".yaml"))


def check(package=None):
    """({test: entry} accepted, [(entry, problem)] rejected) for one package,
    or for all packages when none is named. Tokens must be unique across all."""
    secret = key()
    # GitHub gives fork and Dependabot PRs no secrets. There, an entry that sits
    # unchanged on the base branch (verified when it merged) still counts; a new
    # or edited one can't be verified and is rejected. Fails closed otherwise.
    trusted = None
    base = os.environ.get("EXCEPTIONS_BASE")
    if secret is None and os.environ.get("GITHUB_ACTIONS") == "true" and base:
        trusted = {p: base_entries(base, p) for p in packages()}
        print("::warning::no COVERAGE_EXCEPTIONS_KEY: only exceptions already on the base branch count",
              file=sys.stderr)
    tokens = {}
    for p in packages():
        for e in entries(p):
            if isinstance(e, dict):
                tokens[str(e.get("token"))] = tokens.get(str(e.get("token")), 0) + 1
    accepted, rejected = {}, []
    for p in [package] if package else packages():
        seen = set()
        for e in entries(p):
            if not isinstance(e, dict):
                rejected.append(({"test": str(e)}, "an entry must be a mapping of test, reason, token"))
                continue
            test, reason, token = (str(e.get(f, "")) for f in ("test", "reason", "token"))
            nonce, _, sig = token.partition("-")
            if set(e) != {"test", "reason", "token"}:
                problem = "an entry has exactly three fields: test, reason, token"
            elif secret is None and trusted is None:
                problem = f"no key at {KEY}: no token can be checked"
            elif secret is None and e not in trusted.get(p, []):
                problem = "new or edited, and this run has no key to verify it (a maintainer re-runs it)"
            elif secret is not None and (not nonce or sig != sign(secret, nonce, p, test, reason)):
                problem = "signature doesn't match (not vended, or edited after vending)"
            elif tokens[token] > 1:
                problem = "token used more than once"
            elif test in seen:
                problem = "test listed more than once"
            else:
                problem = None
            if problem:
                rejected.append((e, problem))
            else:
                accepted[test] = e
                seen.add(test)
    return accepted, rejected


def vend(package, test, reason):
    if not reason.strip():
        sys.exit("a reason is required")
    if any(e.get("test") == test for e in entries(package)):
        sys.exit(f"{test} already has an exception in {path(package)}")
    secret = key(create=True)
    nonce = secrets.token_hex(4)
    token = f"{nonce}-{sign(secret, nonce, package, test, reason)}"
    os.makedirs(DIR, exist_ok=True)
    if not os.path.exists(path(package)):
        open(path(package), "w").write(HEADER)
    block = yaml.safe_dump([{"test": test, "reason": reason, "token": token}],
                           sort_keys=False, allow_unicode=True, width=1000)
    with open(path(package), "a") as f:
        f.write("\n" + block)
    print(token)


if __name__ == "__main__":
    if len(sys.argv) == 5 and sys.argv[1] == "vend":
        vend(*sys.argv[2:])
    elif len(sys.argv) in (2, 3) and sys.argv[1] == "check":
        ok, bad = check(sys.argv[2] if len(sys.argv) == 3 else None)
        print(f"{len(ok)} accepted, {len(bad)} rejected")
        for e, problem in bad:
            print(f"  REJECTED {e.get('test')}: {problem}")
        sys.exit(1 if bad else 0)
    else:
        sys.exit(__doc__)
