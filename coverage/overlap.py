"""Overlap analysis of a matrix.py run, shared by report.py and report_html.py.

A test is internal when its source is under src/; clusters and unique counts
are over the blackbox tests (tests/) only.
"""
import json
import os
import re
from types import SimpleNamespace

import config
import exceptions


def load(out, crate, jaccard=0.9):
    crate = os.path.abspath(crate)
    matrix = json.load(open(os.path.join(out, "matrix.json")))
    data = json.load(open(os.path.join(out, "regions.json")))
    regions = [tuple(r) for r in data["regions"]]
    left_out = config.excluded(os.path.basename(crate))
    dropped = {i for i, r in enumerate(regions) if left_out(r[0])}
    all_sets = {k: set(v) - dropped for k, v in data["tests"].items()}

    files = []
    for root, dirs, names in os.walk(crate):
        dirs[:] = [d for d in dirs if d not in ("target", ".git")]
        files += [os.path.join(root, n) for n in names if n.endswith(".rs")]
    texts = {f: open(f, encoding="utf-8").read().splitlines() for f in files}

    def locate(key):
        """(relative file, first line, last line, source lines) of a test
        function, attributes and doc comments included; lines are 1-based."""
        binary, path = key.split("::", 1)
        segments = path.split("::")
        name = segments[-1]
        pattern = re.compile(rf"^\s*(pub(\(crate\))?\s+)?(async\s+)?fn\s+{re.escape(name)}\s*\(")
        best = None
        for f, lines in texts.items():
            for i, line in enumerate(lines):
                if pattern.match(line):
                    rel = os.path.relpath(f, crate)
                    parts = re.split(r"[/.]", rel)
                    score = sum(1 for s in segments[:-1] if s in parts)
                    score += 1 if binary.split("::")[-1] in parts else 0
                    if best is None or score > best[0]:
                        best = (score, rel, i)
        if best is None:
            return ("?", 0, 0, [])
        _, rel, start = best
        lines = texts[os.path.join(crate, rel)]
        top = start
        while top > 0 and lines[top - 1].strip().startswith(("#[", "///", "//")):
            top -= 1
        depth, end, opened = 0, start, False
        for j in range(start, len(lines)):
            depth += lines[j].count("{") - lines[j].count("}")
            opened = opened or "{" in lines[j]
            if opened and depth <= 0:
                end = j
                break
        return (rel, top + 1, end + 1, lines[top:end + 1])

    loc = {k: locate(k) for k in all_sets}
    internal = sorted(k for k in all_sets if loc[k][0].startswith("src/"))
    internal_set = set(internal)
    sets = {k: s for k, s in all_sets.items() if k not in internal_set}

    hits = [0] * len(regions)
    for s in sets.values():
        for r in s:
            hits[r] += 1
    unique = {k: sum(1 for r in s if hits[r] == 1) for k, s in sets.items()}

    def similarity(a, b):
        return len(a & b) / len(a | b) if a | b else 1.0

    keys = sorted(sets)
    parent = {k: k for k in keys}

    def find(k):
        while parent[k] != k:
            parent[k] = parent[parent[k]]
            k = parent[k]
        return k

    for i, a in enumerate(keys):
        for b in keys[i + 1:]:
            sa, sb = sets[a], sets[b]
            if sa and sb and (sa <= sb or sb <= sa or similarity(sa, sb) >= jaccard):
                parent[find(a)] = find(b)

    groups = {}
    for k in keys:
        groups.setdefault(find(k), []).append(k)
    clusters = [sorted(g, key=lambda k: -len(sets[k])) for g in groups.values() if len(g) > 1]

    def strength(c):
        return min(similarity(sets[k], sets[c[0]]) for k in c[1:])

    def candidates(c):
        return sum(1 for k in c if unique[k] == 0)

    clusters.sort(key=lambda c: (candidates(c) == 0, -strength(c), -len(c)))
    clustered = {k for c in clusters for k in c}
    loners = sorted((k for k in keys if unique[k] == 0 and k not in clustered), key=lambda k: -len(sets[k]))

    def relation(k, anchor):
        a, s = sets[anchor], sets[k]
        if k == anchor:
            return "anchor", "largest in cluster"
        if s == a:
            return "identical", "same regions as the anchor"
        if s <= a:
            return "subset", f"{len(s)} of the anchor's {len(a)} regions"
        return "overlap", f"{similarity(s, a):.0%} shared with the anchor"

    def kind(c):
        anchor = sets[c[0]]
        if all(sets[k] == anchor for k in c):
            return "identical"
        return "contained" if all(sets[k] <= anchor for k in c) else "overlapping"

    def where(k):
        f, a, b, _ = loc[k]
        return f"{f}:{a}-{b}"

    def short(key):
        return key.split("::", 1)[1]

    accepted, rejected = exceptions.check(os.path.basename(crate))
    exempt = {k: accepted[short(k)] for k in all_sets if short(k) in accepted}

    return SimpleNamespace(
        exempt=exempt, rejected=rejected,
        excluded=config.patterns(os.path.basename(crate)), dropped=dropped,
        crate=crate, crate_name=os.path.basename(crate), jaccard=jaccard, matrix=matrix,
        regions=regions, texts=texts, loc=loc, internal=internal, sets=sets, keys=keys,
        unique=unique, clusters=clusters, clustered=clustered, loners=loners,
        candidates=candidates, relation=relation, kind=kind, where=where, short=short,
    )
