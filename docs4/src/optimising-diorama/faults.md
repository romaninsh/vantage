# Faults: Errors, Outages & Lies

Each source until now answered, and its answers were true. Production sources are not always honest.
A source can fail one request while the user scrolls. It can stop for ten seconds. It can also
report a row count that it will not serve.

Here the cache stops being an optimisation. It becomes the component that keeps your application
usable.

## A source with faults

```yaml
# datasource/flaky.yaml
type: faker
effect: static
debug: true
count: 1000
seed: 107
shape:
  capabilities: [count, fetch_window, order]
  latency:
    window: 100ms..300ms
  faults:
    error_rate: 0.2      # one request in five fails
    offline: 10s/60s     # <down>/<period>: down for 10s in each 60s
    total_lie: -13       # reports 13 fewer rows than it will serve
```

There are three independent faults, and each one repeats because the shape has a seed.

## What a failure looks like

```text
22:08:05.813  flaky  dio  fetch #1 asks for rows 0..200 — 200 missing, 0 already held
22:08:05.978  flaky  dio  fetch #1 failed after 164ms — shaped source request failed (injected fault)
```

The request identifier connects the two lines. The duration tells you how long you waited for the
failure. The message is the error from the source.

What is **absent** is more important. There is no state line, no cache line, and no total line.

**A failed fetch changes nothing.** The rows that the view already had are still present, still in
their correct positions, and still on screen. A failed refresh costs freshness, not data.

## The viewport causes the retry

Nothing retries on a timer, and this is deliberate. The rows are still absent. The next time that a
view declares a viewport that includes those rows, the loader asks again. A scroll, a repaint, or a
window that gets focus each cause this.

```text
flaky  dio  fetch #2 asks for rows 0..200 — 200 missing, 0 already held
flaky  dio  fetch #2 got 200 rows in 210ms
flaky  cache +200 new, 0 updated — now holding 200 of 987 (20.3%)
flaky  total 987 rows — stated by the source in the same response
```

The retry is a result of demand. It is not a policy. It therefore needs no backoff configuration,
and it cannot flood the source: if no view looks at a range, the loader sends no request for it.

```admonish note title="The one failure that does flood the source"
There is an exception, and the `optimising` project produced it.

A fetch can fail *immediately* and always — a rejected query, and not an unreliable connection. Each
viewport movement then sends a request, each request fails immediately, and the application uses all
available CPU.

During the writing of this guide, a faker datasource advertised `order` while the source refused
every ordered query. Vantage UI appeared to stop responding.

The lesson is not about retry policy. **A capability that a source advertises but cannot do is worse
than a capability that it never claims**, because nothing limits the rate of the failure.
```

## The dishonest total

Read the two last lines above again. The table has **1,000** rows. The source says 987.

The grid uses the total from the source, because the source is the authority on how many rows it
has. One condition overrides this rule, and the next paragraph gives it.

A total that is too small is the safe direction. The grid makes its scrollbar 13 rows short, and the
user cannot reach the last rows.

A total that is too large is the dangerous direction. If a fetch asks for a range with missing rows,
and it fills **none** of them, then the set ends at the first missing row, whatever the total says.
The framework caps the total and reports the cap with the message *"capped: the source promises more
than it serves"*.

Without this rule, the grid shows rows that no fetch can fill. A missing row causes a fetch.
Therefore the application asks the source again, one request each few seconds, for as long as the
page stays open.

## Outages

`offline: 10s/60s` stops the source on a schedule. In the stream, an outage looks like a sequence of
failures. There is no separate offline state to handle, no reconnect procedure, and no separate path
in your code.

- Cached rows continue on screen, and they do not change.
- Ranges that the application has not fetched stay empty. The loader asks for them again when a view
  looks at them.
- When the source returns, the next viewport succeeds and fills the empty rows.

The difficult condition is the first open with an empty cache during an outage. There is nothing to
show and nothing to fetch. This is one more reason to measure the cold run and the warm run
separately.

## What to check under faults

| Question | Where to look |
| --- | --- |
| Did a failure delete data? | No `cache` line after the `failed` line, and rows still on screen |
| Did the retry behave correctly? | One `asks for` line for each viewport movement, and not a sequence of them |
| Is the total correct? | The `total` line, and whether the framework ever caps it |
| Did recovery need a restart? | A `got` line after the failures, with no restart |

An application that handles faults well produces a stream that looks normal: some failures, some
retries when a view looks at the rows, and rows that never disappear.

---

## What we covered

| Item | What it does |
| --- | --- |
| `faults.error_rate` | Makes a fraction of the requests fail |
| `faults.offline` | Stops the source on a schedule, in the form `<down>/<period>` |
| `faults.total_lie` | Changes the number that the grid uses to set its size |
| `failed after …` | The request that did not complete, with its cost and the error from the source |
| *capped: promises more than it serves* | The defence against a total that no fetch can fill |

```admonish tip title="What is next"
The last section makes assertions from what you read. A number that becomes worse then makes a test
fail, instead of nobody noticing.
```
