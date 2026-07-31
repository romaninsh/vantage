# Locking It In: Cache Regression Tests

The numbers that you measured are true of the code as it is today. This section answers a different
question: what happens in six months, when somebody adds a refresh timer or makes a query wider, and
nobody repeats the experiment?

Three measurements automate. The rest is judgement.

| Property | Assertion | Why it becomes worse without a signal |
| --- | --- | --- |
| **Requests for each open** | exactly *N* fetches for a known scroll | a second component that observes the table, a refresh at open, or a second Dio |
| **No repeats** | `repeats == 0` | a total that is too large, or a viewport that moves between two positions |
| **No redundant rows** | `redundant == 0` on a cold run | a refresh that makes too much data invalid |

Put everything else in a report that you read. Do not put it in a test. Latency, memory, and payload
size change for correct reasons, such as a larger fixture or a slower machine. A test that fails on
those numbers gets disabled in one month.

```admonish note title="Assert on counts. Never assert on times."
`3 fetches` is a property of your code. `9.1 seconds` is a property of the machine that ran the
test. Put the first in CI. Put the second in a log that you read when something feels wrong.
```

## What a test needs from the source

**Assertions on exact counts need a source that you control.** A real source gives different numbers
on each run, so you cannot assert on them. You can still point the `optimising` project at a real
source and compare the two reports by hand, as section 1 describes.

```yaml
type: faker
effect: static
debug: true
count: 1000                   # a small set, so the fetch count is small
seed: 103                     # the same rows, in the same sequence, on each run
shape:
  capabilities: [count, fetch_window, order]
  latency:
    window: 20ms              # one value, not a range — no variation in CI
```

There are three changes from the interactive project.

- **`count`** decreases from 200,000 to 1,000, so that a short scroll covers a known part of the set.
- **`seed`** was always present, and now it is necessary. A test that asserts on row 0 needs row 0 to
  be the same row on each machine. A seed fixes the sequence of generated values, so the same seed
  with the same `count` gives the same rows. A different `count` gives a different set.
- **`latency`** becomes one value instead of a range. Variable latency is realistic in a measurement
  session. In a test it causes intermittent failures and gives no benefit.

## The structure of a test

1. **Start from a known cache state.** Delete the cache file of the datasource for a cold run. For a
   warm run, run Vantage UI one time to fill the cache, then measure the second start.
2. **Do a known interaction.** Open one page, declare one viewport, or scroll a fixed number of
   rows. Use `--page=<key>`, so that no other page adds requests to your count.
3. **Wait for the requests to stop.** Do not wait for a fixed time. Poll the fetch count until it
   does not change for 500ms. A fixed `sleep` is slower than necessary, and it also fails on a busy
   CI machine.
4. **Assert on the session summary.** Read the counters, compare them, and fail with a clear
   message.

## How to read the numbers

```sh
# each fetch of the session
grep 'asks for' session.log | wc -l

# the summary, printed when Vantage UI quits
grep -A3 'session summary' session.log
```

In a Rust harness, you read the same numbers directly:

```rust
use vantage_diorama::stats;

// Limit the measurement to this interaction, not to the whole process.
stats::reset_fetch_stats();

// ... drive the page ...

let stats = stats::fetch_stats();
// `table` is the name of the Dio, which is the master table of the datasource.
let people = stats.iter().find(|s| s.table == "people").expect("the table was read");
assert_eq!(people.repeats, 0, "the application fetched a window more than one time");
assert_eq!(people.rows_redundant, 0, "rows arrived that the cache already held");
```

[`reset_fetch_stats`](vantage_diorama::stats::reset_fetch_stats) makes the measurement local to one
test. The counters are global to the process and they accumulate. A test that does not reset them
measures each test before it.

To assert on the count of fetches, first measure the count for your interaction, then write that
number into the test. The count depends on the read-ahead that section 4 describes, so you cannot
calculate it from the number of screens.

## The assertion that finds faults

If you write one assertion only, write this one:

```rust
assert_eq!(people.repeats, 0);
```

It is cheap, it does not fail intermittently, and it finds every repeated fetch.

It does not find a large payload. Section 4 shows a fault with 19.6MB of traffic, zero repeats, and
zero redundant rows. Assert on the payload separately, or read the `payload` line.

## The result

The result is not a faster application. The framework always did this work. The result is an
application that you can **measure**.

---

## What we covered

| Item | What it does |
| --- | --- |
| `seed:` with one latency value | Makes a run repeatable, so that you can assert on it |
| `--page=<key>` | Opens one page, so you count the requests of that page only |
| `stats::reset_fetch_stats()` | Limits the counters to one interaction |
| `stats::fetch_stats()` | Gives the counters in Rust, with no log to parse |
| `repeats == 0` | The one assertion to write, if you write one |

```admonish tip title="Where to go next"
If you want to measure your own source, point the `optimising` project at it. The method works with
any datasource, and the faker runs stay useful as the control.

If you write a driver, [Adding a New Persistence](../new-persistence.md) is where you select the
capabilities to advertise. This chapter is where you find out whether your driver does what it
advertised.
```
