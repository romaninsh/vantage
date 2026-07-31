# Optimising Live Diorama Data

An application reports its faults. It does not report its inefficiencies. A failed request raises an
error, but a duplicate fetch stays unnoticed. Typical problems are:

- refetch a static page in its entirety every 5 seconds
- fetch all columns of a related table, just to fill a select field in a form
- sort 120,000 rows in the client, and ignore the `sort` capability of the source

Inefficiencies show as extra latency, high CPU usage, or network traffic. This chapter gives you the
mechanisms to detect them and to avoid them.

```admonish info title="Why live data needs a cache"
A user interface repaints many times each second. It cannot wait for the network on each frame, so
the rows must already be in memory. When the user scrolls, the framework fetches rows ahead of the
viewport to keep the movement smooth.

Diorama keeps those rows in a [redb](https://www.redb.org/) file on disk, one file for each
datasource. The rows therefore survive a move to another page, and they survive a restart.

A user interface is not the only consumer of a cache. Vantage is also designed for facade APIs and
for live caches at the edge.

The goal is the same in each case: answer most requests without a request to the master source, and
keep the local copy in agreement with that source. A correct cache answers immediately, and then
pushes each change to the viewport when it arrives.

<svg viewBox="0 0 720 172" width="100%" xmlns="http://www.w3.org/2000/svg" role="img"
     aria-label="Consumers on the left declare a viewport to the Diorama cache in the middle, and receive the live updates that are relevant to it. The cache sends few batched fetches to the master source on the right, which returns changes for the whole table.">
  <defs>
    <marker id="dio-arrow" viewBox="0 0 10 10" refX="9" refY="5"
            markerWidth="7" markerHeight="7" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <g fill="none" stroke="currentColor" stroke-opacity="0.45">
    <rect x="14" y="42" width="122" height="112" rx="6"/>
    <rect x="578" y="62" width="128" height="72" rx="6"/>
  </g>
  <rect x="292" y="52" width="152" height="92" rx="6" fill="none" stroke="#4a90d9" stroke-width="1.6"/>
  <g fill="currentColor" font-family="system-ui, -apple-system, sans-serif" text-anchor="middle">
    <text x="75" y="76" font-size="11.5">UI viewport</text>
    <text x="75" y="102" font-size="11.5">Facade API</text>
    <text x="75" y="128" font-size="11.5">Edge cache</text>
    <text x="368" y="91" font-size="13">Diorama cache</text>
    <text x="368" y="109" font-size="10.5" fill-opacity="0.65">redb, on disk</text>
    <text x="642" y="93" font-size="13">Master source</text>
    <text x="642" y="111" font-size="10.5" fill-opacity="0.65">slow · remote</text>
    <g font-size="10.5" fill-opacity="0.75">
      <text x="214" y="76">declare viewport</text>
      <text x="214" y="134">relevant live updates</text>
      <text x="511" y="76">few fetches, batched</text>
      <text x="511" y="134">whole-table changes</text>
    </g>
  </g>
  <g fill="none" stroke="currentColor" stroke-opacity="0.75" marker-end="url(#dio-arrow)">
    <path d="M140,84 L288,84"/>
    <path d="M288,116 L142,116"/>
    <path d="M448,84 L574,84"/>
    <path d="M574,116 L450,116"/>
  </g>
</svg>
```

## What slows the app down?

| Inefficiency | What happens | The damage |
| --- | --- | --- |
| **Repeated fetch** | The application asks for a window that it already asked for. | One request for each timer tick or repaint |
| **Redundant rows** | Rows arrive that the cache already holds. | Bandwidth, with no change on screen |
| **Off-screen updates** | The source announces a change to a row that the user cannot see, and the view repaints. | CPU usage for data that nobody looks at |
| **Undeclared columns** | Each row carries fields that no table lists. | Bandwidth. A grid of five columns can cost megabytes for each screen. |
| **Work in the client** | The client sorts, searches, or counts locally. | CPU usage, and an answer that covers the loaded rows only |
| **A full copy of a large table** | The source cannot serve windows, so the client reads all the rows at open. | Memory, and a wait before the first screen |
| **A total that no fetch can fill** | The source states more rows than it serves. | Requests for the missing rows, for as long as the page stays open |

There is no best caching strategy. Each use case needs a different approach. Vantage lets you design
and implement your own strategy, but you must first understand how the cache behaves and how to
monitor it.

Vantage UI works well with its default behaviour. This chapter starts from that default, and then
adjusts it.

---

## Overview

| Section | What you do | What it answers |
| --- | --- | --- |
| [1. The setup](./optimising-diorama/stack.md) | Build the `optimising` project and open it in Vantage UI | How do I put a source that I control under a real client? |
| [2. Reading the Debug Stream](./optimising-diorama/debug-stream.md) | Make the source slow, empty the cache, and switch on debug for one datasource | What does my application ask for? |
| [3. Where the Work Happens](./optimising-diorama/capabilities.md) | Remove capabilities and find where the work moves | Which work moved into my process, and what did it cost? |
| [4. Fetching What You Drop](./optimising-diorama/waste.md) | Find repeated fetches, redundant rows, and columns that no view shows | How many of these bytes did anybody need? |
| [5. Faults: Errors, Outages & Lies](./optimising-diorama/faults.md) | Inject failures, outages, and dishonest totals | Does the application continue, or does it stop? |
| [6. Locking It In](./optimising-diorama/regression.md) | Make assertions from your measurements, over a run with a fixed seed | Will I know when this becomes worse? |

**Start here:** [The setup](./optimising-diorama/stack.md)
