# Root CHANGELOG.md ends at 0.2.0 (2025-02-16) — three releases behind

- **Severity:** medium
- **Category:** omissions
- **Location:** `CHANGELOG.md:5`

The repository-level changelog's newest entry is `[0.2.0] - 2025-02-16` and it was last committed on that date. Since then the project shipped 0.3 (tag v0.3.0), the 0.4 rewrite the README/book advertise, and 0.5.x crate releases (vantage-table 0.5.7, vantage-sql 0.5.9, etc.). Per-crate changelogs exist (e.g. `vantage-table/CHANGELOG.md` documents 0.5.7), so the root file is not just incomplete — it actively misrepresents project activity to anyone who opens it from GitHub's front page.

```
# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2025-02-16
```

**Recommendation:** Either bring the root changelog up to date with 0.3/0.4/0.5 milestone summaries (the book's `history.md` already has this content), or replace its body with a pointer to the per-crate CHANGELOG.md files and the book's Historical Timeline.
