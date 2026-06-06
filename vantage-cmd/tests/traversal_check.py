#!/usr/bin/env python3
"""Live cross-check of vantage-cmd relationship traversals against raw `aws`.

For each relation we pick a real parent, traverse it through the example
binary (`--format=json`), then run the equivalent raw `aws` call and compare
the record sets. Not a unit test — needs valid AWS creds; invoked manually.
"""
import json
import os
import subprocess
import sys

BIN = os.path.join(os.path.dirname(__file__), "..", "target", "debug", "examples", "aws-cli")
REGION = "eu-west-1"

PASS, FAIL = 0, 0


def env(profile):
    e = dict(os.environ)
    e["AWS_PROFILE"] = profile
    e["AWS_REGION"] = REGION
    return e


def cli(profile, args):
    """Run the example, return dict {id: record} (or None on error)."""
    p = subprocess.run(
        [BIN, "--format=json", *args],
        capture_output=True, text=True, env=env(profile),
    )
    if p.returncode != 0:
        return None, p.stderr.strip()
    try:
        return json.loads(p.stdout or "{}"), None
    except json.JSONDecodeError as e:
        return None, f"bad json: {e}: {p.stdout[:200]}"


def aws(profile, args):
    p = subprocess.run(
        ["aws", *args, "--output", "json"],
        capture_output=True, text=True, env=env(profile),
    )
    if p.returncode != 0:
        return None, p.stderr.strip()
    return json.loads(p.stdout or "{}"), None


def check(name, got_ids, want_ids):
    global PASS, FAIL
    got, want = set(got_ids), set(want_ids)
    if got == want:
        PASS += 1
        print(f"  PASS  {name}: {len(got)} records match raw aws")
    else:
        FAIL += 1
        print(f"  FAIL  {name}: cmd={len(got)} aws={len(want)}")
        print(f"        only-cmd: {list(got - want)[:3]}")
        print(f"        only-aws: {list(want - got)[:3]}")


def first_key(d):
    return next(iter(d)) if d else None


def test_account(profile):
    print(f"\n========== account {profile} ==========")

    # ---- lambda.functions -> :versions / :aliases -----------------------
    fns, err = cli(profile, ["lambda.functions"])
    if fns:
        fn = first_key(fns)
        print(f"[lambda] parent function: {fn}")
        got, e = cli(profile, ["lambda.function", f"FunctionName={fn}", ":versions"])
        raw, e2 = aws(profile, ["lambda", "list-versions-by-function", "--function-name", fn])
        if got is not None and raw is not None:
            check("lambda.function :versions", got.keys(),
                  [v["Version"] for v in raw["Versions"]])
        else:
            print(f"  SKIP  versions ({e or e2})")
        got, e = cli(profile, ["lambda.function", f"FunctionName={fn}", ":aliases"])
        raw, e2 = aws(profile, ["lambda", "list-aliases", "--function-name", fn])
        if got is not None and raw is not None:
            check("lambda.function :aliases", got.keys(),
                  [a["Name"] for a in raw.get("Aliases", [])])
        else:
            print(f"  SKIP  aliases ({e or e2})")
    else:
        print(f"[lambda] SKIP (no functions: {err})")

    # ---- log.groups -> :streams -----------------------------------------
    groups, err = cli(profile, ["log.groups"])
    if groups:
        g = first_key(groups)
        print(f"[logs] parent group: {g}")
        got, e = cli(profile, ["log.group", f"logGroupName={g}", ":streams"])
        raw, e2 = aws(profile, ["logs", "describe-log-streams", "--log-group-name", g])
        if got is not None and raw is not None:
            check("log.group :streams", got.keys(),
                  [s["logStreamName"] for s in raw["logStreams"]])
        else:
            print(f"  SKIP  streams ({e or e2})")
    else:
        print(f"[logs] SKIP (no groups: {err})")

    # ---- ecs.clusters -> :services / :tasks -----------------------------
    clusters, err = cli(profile, ["ecs.clusters"])
    if clusters:
        c = first_key(clusters)
        print(f"[ecs] parent cluster: {c}")
        got, e = cli(profile, ["ecs.cluster", f"clusterArn={c}", ":services"])
        raw, e2 = aws(profile, ["ecs", "list-services", "--cluster", c])
        if got is not None and raw is not None:
            check("ecs.cluster :services", got.keys(), raw["serviceArns"])
        else:
            print(f"  SKIP  services ({e or e2})")
        got, e = cli(profile, ["ecs.cluster", f"clusterArn={c}", ":tasks"])
        raw, e2 = aws(profile, ["ecs", "list-tasks", "--cluster", c])
        if got is not None and raw is not None:
            check("ecs.cluster :tasks", got.keys(), raw["taskArns"])
        else:
            print(f"  SKIP  tasks ({e or e2})")
    else:
        print(f"[ecs] SKIP (no clusters: {err})")

    # ---- s3.buckets -> :objects (pick a non-empty, in-region bucket) ----
    buckets, err = cli(profile, ["s3.buckets"])
    if buckets:
        picked = None
        for b in buckets:
            if REGION not in b:
                continue
            raw, e2 = aws(profile, ["s3api", "list-objects-v2", "--bucket", b, "--max-items", "200"])
            if raw is not None and raw.get("Contents"):
                picked = (b, raw)
                break
        if picked:
            b, raw = picked
            print(f"[s3] parent bucket: {b}")
            got, e = cli(profile, ["s3.bucket", f"Name={b}", ":objects"])
            if got is not None:
                # cmd applies no --max-items; compare against full listing.
                rawfull, _ = aws(profile, ["s3api", "list-objects-v2", "--bucket", b])
                check("s3.bucket :objects", got.keys(),
                      [o["Key"] for o in rawfull.get("Contents", [])])
            else:
                print(f"  SKIP  objects ({e})")
        else:
            print("[s3] SKIP (no non-empty in-region bucket)")
    else:
        print(f"[s3] SKIP (no buckets: {err})")

    # ---- iam.users -> :groups / :access_keys / :policies ----------------
    users, err = cli(profile, ["iam.users"])
    if users:
        u = first_key(users)
        print(f"[iam] parent user: {u}")
        got, e = cli(profile, ["iam.user", f"UserName={u}", ":groups"])
        raw, e2 = aws(profile, ["iam", "list-groups-for-user", "--user-name", u])
        if got is not None and raw is not None:
            check("iam.user :groups", got.keys(),
                  [g["GroupName"] for g in raw["Groups"]])
        got, e = cli(profile, ["iam.user", f"UserName={u}", ":access_keys"])
        raw, e2 = aws(profile, ["iam", "list-access-keys", "--user-name", u])
        if got is not None and raw is not None:
            check("iam.user :access_keys", got.keys(),
                  [k["AccessKeyId"] for k in raw["AccessKeyMetadata"]])
        got, e = cli(profile, ["iam.user", f"UserName={u}", ":policies"])
        raw, e2 = aws(profile, ["iam", "list-attached-user-policies", "--user-name", u])
        if got is not None and raw is not None:
            check("iam.user :policies", got.keys(),
                  [p["PolicyName"] for p in raw["AttachedPolicies"]])
    else:
        print(f"[iam] SKIP (no users: {err})")


if __name__ == "__main__":
    profiles = sys.argv[1:] or ["489109585615_ECPDeveloper"]
    for prof in profiles:
        test_account(prof)
    print(f"\n==== {PASS} passed, {FAIL} failed ====")
    sys.exit(1 if FAIL else 0)
