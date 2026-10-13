#!/usr/bin/env python3
"""Generate llms.txt and llms-full.txt for the framework book.

Run after `mdbook build`:  python3 script/book-llms.py docs4

Reads docs4/src/SUMMARY.md for chapter order and copies each chapter's
markdown next to its rendered page (intro/step1.md beside intro/step1.html),
so the links in llms.txt resolve to raw markdown. Follows https://llmstxt.org.
"""

import re
import shutil
import sys
from pathlib import Path

BASE_URL = "https://vantage-ui.com/book/"

SUMMARY = (
    "Vantage is an open-source Rust framework for business data: typed "
    "entities and tables over SQL (PostgreSQL, MySQL, SQLite), SurrealDB, "
    "MongoDB, REST/GraphQL APIs and more, with relations, expressions, "
    "Rhai scripting and live (push or poll) data. It is the engine under "
    "Vantage UI (https://vantage-ui.com/). This book teaches it bottom-up, "
    "from a first query to live APIs and adding a new persistence."
)

ENTRY = re.compile(r"^(\s*)- \[([^\]]+)\]\(\./([^)]+\.md)\)")


def chapters(src):
    """(depth, title, path) for every SUMMARY.md entry, in book order."""
    for line in (src / "SUMMARY.md").read_text().splitlines():
        m = ENTRY.match(line)
        if m:
            yield len(m[1]) // 2, m[2], m[3]


def lede(md, limit=220):
    """First prose paragraph, flattened: the chapter's one-line description."""
    md = re.sub(r"^```.*?^```", "", md, flags=re.S | re.M)
    for block in re.split(r"\n\s*\n", md):
        block = block.strip()
        if not block or block[0] in "#<|>!-*{[":
            continue
        text = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", block)  # [text](url)
        text = re.sub(r"\[`?([^\]`]+)`?\]", r"\1", text)         # rustdoc [`Type`]
        text = re.sub(r"\s+", " ", text).replace("**", "")
        return text if len(text) <= limit else text[: limit - 1].rsplit(" ", 1)[0] + "…"
    return ""


def main(book_dir):
    book_dir = Path(book_dir)
    src, out = book_dir / "src", book_dir / "book"

    entries = []  # (depth, title, md_url, html_url, description, markdown)
    for depth, title, path in chapters(src):
        md = (src / path).read_text()
        dest = out / path
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src / path, dest)
        html = path[: -len(".md")] + ".html"
        entries.append((depth, title, BASE_URL + path, BASE_URL + html, lede(md), md))

    lines = [
        "# Vantage Framework", "",
        f"> {SUMMARY}", "",
        f"Each link below is a chapter's markdown source. The whole book in one file: {BASE_URL}llms-full.txt", "",
    ]
    # A top-level chapter with sub-chapters becomes its own section; the
    # standalone ones are gathered under "Topics", keeping book order.
    sections, topics = [], []
    for i, (depth, title, md_url, _, desc, _) in enumerate(entries):
        line = f"- [{title}]({md_url})" + (f": {desc}" if desc else "")
        if depth > 0:
            sections[-1][1].append(line)
        elif i + 1 < len(entries) and entries[i + 1][0] > 0:
            sections.append((title, [line]))
        else:
            topics.append(line)
    for title, rows in sections:
        lines += [f"## {title}", "", *rows, ""]
    if topics:
        lines += ["## Topics", "", *topics, ""]
    lines += [
        "## Optional", "",
        "- [Vantage UI](https://vantage-ui.com/llms.txt): the AI-first app builder built on this framework",
        "- [Source on GitHub](https://github.com/romaninsh/vantage): the vantage-* crates",
        "",
    ]
    (out / "llms.txt").write_text("\n".join(lines))

    full = [f"# Vantage Framework\n\n> {SUMMARY}\n"]
    full += [f"---\n\nSource: {html_url}\n\n{md.strip()}\n" for *_, html_url, _, md in entries]
    (out / "llms-full.txt").write_text("\n".join(full))

    size = sum(len(e[5]) for e in entries) // 1024
    print(f"llms.txt: {len(entries)} chapters; llms-full.txt: {size} KiB")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "docs4")
