---
name: matrix-based-test-cleanup
description: Use when the coverage matrix or the CI duplicate-test check reports overlapping, identical or near-identical tests in a vantage crate, or when a crate's tests should be reduced to one readable test per behaviour.
---

# Matrix-based test cleanup

The coverage matrix says which tests run the same code; it can't say whether they check the same
behaviour. Two agents settle that between themselves: a **proposer** writes each change, a
**challenger** holds it to [rules.md](rules.md). The coordinator (you) runs every tool and only
brings the user what the agents can't settle.

Scope: **one crate per run.**

## Non-negotiables

Every test that survives:
- reads plainly: one setup, one call, plain asserts; no helpers, loops or tables a few lines replace;
- checks simple, named cases with exact expected values from the contract;
- uses only the crate's public API;
- answers, for every case in it and every test next to it, **how is this different from the
  other one?**, and its asserts show that difference. No answer: it goes.

## Process

1. **Baseline, then groups.** `coverage/matrix.sh --baseline <crate>`, then
   `python3 -I coverage/matrix-rs/skills/matrix-based-test-cleanup/groups.py <crate>`, which
   writes `coverage/report/<crate>/groups.md`.
2. **Dispatch** one proposer ([proposer.md](proposer.md)) and one challenger
   ([challenger.md](challenger.md)) for the crate, with a workspace directory for their files.
   Keep both agents for the whole run: every later turn goes to the same agent (SendMessage).
3. **Let them settle.** The proposer writes `proposals/<n>.md` for every group. The challenger
   answers each with `<n>.verdict.md`. Relay verdicts and revisions between them without comment
   until every proposal is `ACCEPT`. When they agree a rule rules.md lacks, the challenger adds it
   to rules.md under "Agreed in runs" with the case.
4. **Escalate only** a group still not accepted after 3 rounds, or a `DISPUTE` the challenger
   upholds. Put it to the user in a few lines: the two positions, the code in question.
5. **Apply and check.** The proposer applies the accepted proposals. You run the crate's tests
   with `--all-features`, then `coverage/matrix.sh <crate>`: `lost.txt` must be within the crate's
   tolerance. A failure or a loss goes back to the proposer as a demand.
6. **Report:** tests before and after, regions before and after, rules added, anything escalated.

## Roles

| Role | Does | Never |
|---|---|---|
| Coordinator | runs the matrix, groups.py, cargo, git; relays; escalates | writes test changes; argues either side |
| Proposer | writes and revises proposals, applies accepted ones | runs tools; argues instead of revising |
| Challenger | checks proposals against rules.md; ACCEPT or DEMAND; records agreed rules | writes or sketches the fix; accepts on its own reasoning; runs tools |
