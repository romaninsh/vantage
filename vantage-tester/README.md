# vantage-tester

The engine behind Vantage's what-if scenario tests. It reads
`tests/scenarios/*.yaml`, plans the tree of outcomes, walks it through a
`ScenarioHost` and reports green, yellow or red, with JUnit output for CI.

It knows no backend. Rhai in `before`, `assert`, `do` and `vars.from` is
carried as text; the host evaluates it, owns the data and forks it. The
Vantage host over in-memory datasources lives in vantage-backend.

```toml
[dependencies]
vantage-tester = "0.1.0"
```

## Scenario files

```yaml
fixtures: acme-part-funded
vars:
  acme: { record: loan, id: "loan:acme" }

scenarios:
  funded:
    session: { app: portal, investor_id: $carol }
    clock: 2026-11-14T09:00:00Z
    steps:
      - step: Carol places the bid that fills the loan
        do: { action: place_bid, args: { loan: $acme, amount: 2000, rate: 7 } }
        assert: [result.bid != ()]
        what_if: [no-cash, revoked]
      - step: Admin closes the auction two days later
        clock: +2d
        as: desktop
        before: |
          set(acme, #{ bid_total: 10000 })
        do: { record: $acme, action: close_auction }

  no-cash:
    steps:
      - step: Carol has not enough funds
        before: |
          set(carol, #{ cash: 100 })
        fails: "Not enough cash"
```

Write Rhai that contains `#{` as a `|` block or quote it: in YAML, ` #`
starts a comment. `plan` refuses a script that was cut short this way when its
brackets don't close or it ends in a comma; a cut that leaves balanced text,
like `x == #{ a: 1 }` read as `x ==`, gets through to the host's Rhai parser.

- A scenario named in a `what_if` starts from the state just before that step.
  Its first step replaces that step and takes whatever it leaves out of
  `clock`, `as`, `before`, `do`, `stub` and `assert` (`assert` not when it has
  `fails:`). `vars` and `what_if` stay its own.
- `revoked` above is named but not written, so the file is yellow. So is
  `what_if: true`. A scenario with `skip: <reason>` settles its what-if.
- `extends: <scenario>` takes another scenario's settings and steps, overrides
  the settings it names and appends its own steps.
- Step clock moves: `+90m`, `+3h`, `+2d`, `+1w`, `+1mo`. The `+` is required.

## Running

```rust
use std::path::Path;

use vantage_tester::{Report, load, plan, walk};

let file = load(project_root, Path::new("tests/scenarios/funding.yaml"))?;
let tree = plan(&file)?;
let report = walk(&host, &file, &tree, &|event| eprintln!("{event:?}")).await;
let junit = Report { files: vec![report] }.to_junit();
```

`walk` calls `host.load` once per file (fixtures and file-level vars), then
starts each root scenario with `host.root` over that base and a seed hashed from
the file's project-relative path and the scenario's name (`vantage_tester::seed`),
so ids repeat on every machine. It forks the state before each step with
what-ifs, so the steps above run once, and before each `fails:` step, so a
failed step keeps nothing. A harness error (`StepFailure::harness`: unknown
action, bad `$var`, missing stub) is always red, even under `fails:`.
