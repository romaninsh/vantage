# Changelog

## 0.1.0 — 2026-11-14

- Scenario file model: `load(root, path)`, `parse`, `ScenarioFile`, `Scenario`, `Step`, `VarDef`,
  `Session`, `Do`, `WhatIf`, `ClockOffset`. Unknown keys are refused; the stored path is
  project-relative and normalised (`..` resolved), and a file outside the root is an error.
- `plan` builds the what-if `Tree`: `extends` resolved, what-ifs anchored at the step naming them,
  first-step inheritance of `clock`, `as`, `before`, `do`, `stub` and `assert`, `Branch::Missing`
  for an unwritten what-if, `Tree::step_count`. Refuses `extends` and what-if cycles, duplicate
  step sentences, a what-if that sets its own `session`, `clock` or `extends`, a step variable
  shadowing a file variable, an empty `fails:`, and Rhai that YAML cut short at ` #` (comments
  skipped). Errors name the file and scenario.
- `ScenarioHost` trait (`load` once per file, `root`, async fallible `fork`, `advance`, `before`,
  `run`, `assert`, `declare`) and `walk`: forks before each what-if and each `fails:` step, runs
  the steps above a what-if once, and reports green, yellow or red with each step's result and
  writes. Harness errors (`StepFailure::harness`) never satisfy `fails:`. After a red step, later
  what-if subtrees are reported with every step not run. Every step, run or not, emits
  `Event::StepFinished`, so a progress counter reaches `FileStarted`'s total.
- `StepReport::who`: who the step ran as, from `Session::label` (`desktop`, or the session's
  values and `@api`: `Marta · manager · south @staff`); absent when no session is named.
- `seed(path, scenario)`: stable FNV-1a seed per root scenario.
- `Report::to_junit`, with a skipped case per unwritten what-if.
