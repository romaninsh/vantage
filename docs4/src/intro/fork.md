# Choose Your Path

You now have a **Vista** — a runtime handle over your data that works the same no matter what's
behind it. Everything up to here was the foundation. Everything past here is the **reactive
stack**: a local cache, an event bus, and watchable views that update as the data changes.

That stack is backend-agnostic, and the rest of the guide proves it by forking here into two
paths. Both build the *same* layers — Dio, Lens, Augmentation, Scenery, and a watch-streaming
HTTP server — and differ only in the backend underneath. Pick the one shaped like your problem;
the code you write is nearly identical either way.

## Which one is yours?

```admonish example title="Path A · A facade over an API you don't control"
<svg viewBox="0 0 624 76" xmlns="http://www.w3.org/2000/svg" font-family="sans-serif" font-size="12.5" fill="currentColor" role="img" aria-label="A React or terminal UI reads from a cache, backed by a Vista over the slow read-only AWS S3 API.">
  <defs><marker id="pa-arw" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0 L10 5 L0 10 z" fill="#888"/></marker></defs>
  <rect x="6" y="16" width="130" height="46" rx="7" fill="#5b8cb0" fill-opacity="0.18" stroke="#5b8cb0" stroke-width="1.5"/>
  <text x="71" y="35" text-anchor="middle">React / TUI</text>
  <text x="71" y="51" text-anchor="middle" font-size="11" opacity="0.6">ratatui, web</text>
  <line x1="166" y1="39" x2="140" y2="39" stroke="#888" stroke-width="1.8" marker-end="url(#pa-arw)"/>
  <rect x="170" y="16" width="130" height="46" rx="7" fill="#c08a4a" fill-opacity="0.2" stroke="#c08a4a" stroke-width="2.2"/>
  <text x="235" y="35" text-anchor="middle" font-weight="bold">Cache</text>
  <text x="235" y="51" text-anchor="middle" font-size="11" opacity="0.6">Dio · events</text>
  <line x1="330" y1="39" x2="304" y2="39" stroke="#888" stroke-width="1.8" marker-end="url(#pa-arw)"/>
  <rect x="334" y="16" width="130" height="46" rx="7" fill="#a98a6a" fill-opacity="0.18" stroke="#a98a6a" stroke-width="1.5"/>
  <text x="399" y="35" text-anchor="middle">Vista</text>
  <text x="399" y="51" text-anchor="middle" font-size="11" opacity="0.6">AWS S3</text>
  <line x1="494" y1="39" x2="468" y2="39" stroke="#888" stroke-width="1.8" marker-end="url(#pa-arw)"/>
  <rect x="490" y="16" width="130" height="46" rx="7" fill="#5aa06f" fill-opacity="0.28" stroke="#5aa06f" stroke-width="1.8"/>
  <text x="555" y="35" text-anchor="middle" font-weight="bold">AWS API</text>
  <text x="555" y="51" text-anchor="middle" font-size="11" opacity="0.6">slow, read-only</text>
</svg>

Your data lives behind something slow you cannot change — a cloud API, a legacy service, a
third-party endpoint. It is read-only, hundreds of milliseconds away, and cannot sort, search, or
paginate. You want a fast, queryable local view of it.

The example builds an inventory of a public **S3** bucket (NOAA's climate archive) — thousands of
files served seamlessly and responsively. Diorama caches the listing, **injects the capabilities
S3 lacks**, and enriches each row from its contents; the path ends with a terminal UI (ratatui)
and an Axum API feeding a React frontend.

**[Take Path A →](./step5-dio-lens.md)** — steps 5–8.
```

```admonish tip title="Path B · A live view in front of your own database"
<svg viewBox="0 0 624 76" xmlns="http://www.w3.org/2000/svg" font-family="sans-serif" font-size="12.5" fill="currentColor" role="img" aria-label="A React or terminal UI reads from a cache, backed by a Vista over a PostgreSQL database you control.">
  <defs><marker id="pb-arw" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0 L10 5 L0 10 z" fill="#888"/></marker></defs>
  <rect x="6" y="16" width="130" height="46" rx="7" fill="#5b8cb0" fill-opacity="0.18" stroke="#5b8cb0" stroke-width="1.5"/>
  <text x="71" y="35" text-anchor="middle">React / TUI</text>
  <text x="71" y="51" text-anchor="middle" font-size="11" opacity="0.6">ratatui, web</text>
  <line x1="166" y1="39" x2="140" y2="39" stroke="#888" stroke-width="1.8" marker-end="url(#pb-arw)"/>
  <rect x="170" y="16" width="130" height="46" rx="7" fill="#c08a4a" fill-opacity="0.2" stroke="#c08a4a" stroke-width="2.2"/>
  <text x="235" y="35" text-anchor="middle" font-weight="bold">Cache</text>
  <text x="235" y="51" text-anchor="middle" font-size="11" opacity="0.6">Dio · events</text>
  <line x1="330" y1="39" x2="304" y2="39" stroke="#888" stroke-width="1.8" marker-end="url(#pb-arw)"/>
  <rect x="334" y="16" width="130" height="46" rx="7" fill="#a98a6a" fill-opacity="0.18" stroke="#a98a6a" stroke-width="1.5"/>
  <text x="399" y="35" text-anchor="middle">Vista</text>
  <text x="399" y="51" text-anchor="middle" font-size="11" opacity="0.6">Postgres</text>
  <line x1="494" y1="39" x2="468" y2="39" stroke="#888" stroke-width="1.8" marker-end="url(#pb-arw)"/>
  <rect x="490" y="16" width="130" height="46" rx="7" fill="#5aa06f" fill-opacity="0.28" stroke="#5aa06f" stroke-width="1.8"/>
  <text x="555" y="35" text-anchor="middle" font-weight="bold">PostgreSQL</text>
  <text x="555" y="51" text-anchor="middle" font-size="11" opacity="0.6">you control it</text>
</svg>

Your data lives in a **relational database you own** — you read it, write it, and it already
sorts, searches, and joins. What you want is a *live, cached, watchable facade* in front of it:
instant reads, changes streaming to every viewer, writes routed on your terms.

The example builds a bar's product inventory whose stock ticks down in real time as items sell.
The same caching, reactive, watch-streaming stack — and the path ends by moving the app from
**SQLite to PostgreSQL** with a single switch, proving the backend was never load-bearing.

**[Take Path B →](./step5-sql-dio.md)** — steps 5–7.
```

## Why the guide leads with S3

If you own a SQL database, an S3 bucket may look like a strange place to *start* teaching caching
and reactivity. That is deliberate. A slow, read-only, capability-poor backend makes the value of
the layer above **visible**: every millisecond the cache saves, and every sort the backend cannot
do but Diorama can, is obvious. A capable SQL database would hide the very problem the reactive
stack solves — so Path A teaches it where it shows, and Path B proves it was never about S3.

|                    | Path A — Custom API (S3)                  | Path B — Relational DB (SQL)      |
| ------------------ | ----------------------------------------- | --------------------------------- |
| Backend            | slow, remote, read-only                   | fast, local, writable             |
| Sort / search      | no — Diorama injects it                    | yes — natively                    |
| Writes             | routed elsewhere (the master can't take them) | routed to the database        |
| Reactivity source  | periodic reconcile (S3 can't push)        | your own writes + reconcile       |
| The payoff         | a rich handle over a poor backend         | a live facade, then a backend swap |

Both paths converge on the same idea: **once a Dio wraps a Vista, nothing above it knows or cares
what the backend is.** Read whichever path is yours — or both, and watch the code stay the same
while the backend changes underneath it.
