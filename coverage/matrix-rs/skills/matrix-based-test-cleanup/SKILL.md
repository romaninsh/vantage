---
name: matrix-based-test-cleanup
description: Use when the coverage matrix or the CI duplicate-test check reports overlapping, identical or near-identical tests in a vantage crate, or when a crate's tests should be reduced to one readable test per behaviour.
---

# Matrix-based test cleanup

## Overview

The coverage matrix says which tests run the same code. It can't say whether they check the same
behaviour. This skill turns its overlap groups into fewer, simpler tests through two agents: a
**proposer** who writes each change and a **challenger** who holds it to the rules in
[rules.md](rules.md). You, the coordinator, run the matrix and cargo, pass work between them, and
settle what they can't.

Scope: **one crate per run.** Pilot new rules on a small crate first.

## Non-negotiables

Every test that survives:
- is readable: one setup, one call, then plain asserts; no helpers, loops or tables a few lines can replace;
- checks a simple, named case with an exact expected value from the contract;
- uses only the crate's public API;
- answers, for every case in it and every test next to it: **how is this different from the other one?**
  No answer means it goes.

## Roles

| Role | Does | Never |
|---|---|---|
| Coordinator (you) | runs `coverage/matrix.sh`, `groups.py`, cargo; dispatches; relays; applies rule changes the user approves | writes test changes itself |
| Proposer | reads one group, writes the change as a patch proposal, revises when challenged | runs the matrix or cargo; argues instead of revising |
| Challenger | checks each proposal against rules.md and the non-negotiables; returns ACCEPT or a demand | writes code; justifies or rescues a proposal; runs the matrix or cargo |

The challenger states what is wrong and which rule it breaks, and the proposer fixes it. The
challenger never supplies the justification or the fix itself.

## Process

1. **Matrix.** `coverage/matrix.sh <crate>`, then
   `python3 -I coverage/matrix-rs/skills/matrix-based-test-cleanup/groups.py <crate>` writes
   `coverage/report/<crate>/groups.md`: identical and ≥98%-shared groups with sources. The CI
   job summary's "possible duplicates" list for the crate is the same input.
2. **Dispatch** one proposer ([proposer.md](proposer.md)) and one challenger
   ([challenger.md](challenger.md)) for the crate. Keep both agents for the whole run: every later
   round goes to the same agent through SendMessage, so each keeps its context.
3. **Per group, loop:**
   1. Proposer writes `proposals/<n>.md`: the tests as they are, the change (DELETE / COMBINE /
      KEEP) with the full new test code, and for every case kept, how it differs from the others.
   2. Challenger answers in `proposals/<n>.verdict.md`: `ACCEPT`, or `DEMAND: <what is wrong> —
      <rule>`. The challenger can also write `NEW RULE?: <case it doesn't cover> — <proposed rule>`.
   3. On a DEMAND, send it to the proposer, who revises. **At most 3 rounds**, then the group goes
      to the user with both positions.
4. **New rules.** Collect every `NEW RULE?` and put them to the user. Only user-approved rules
   go into rules.md, with the case that made them.
5. **Apply.** The proposer applies the accepted proposals to the test files (Edit only). You then
   run the crate's tests by name with `--all-features`, re-run the matrix, and check `lost.txt`
   stays within the crate's tolerance. Lost coverage goes back to the proposer as a demand.
6. **Report** to the user: tests before and after, the groups escalated, new rules proposed.

## Common mistakes

| Mistake | Fix |
|---|---|
| Combining by adding a helper, a loop or a table | One setup and one call carrying all cases, plain asserts (rules.md R2) |
| Folding a weaker input into the combined test | Keep the strongest input of a class and drop the weaker (R4) |
| Judging only the tests in the group | Search the whole file, and the crate, for tests of the same behaviour (R6) |
| Keeping two inputs that take the same code path | Read the code's branches to decide sameness, never expected values (R5) |
| The challenger arguing for a change it wants | It states the problem and the rule; the proposer writes the fix |
| Agents re-running the matrix to check themselves | Only the coordinator runs the matrix and cargo |
