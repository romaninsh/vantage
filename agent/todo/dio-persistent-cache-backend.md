# Persistent CacheBackend for Dio

**Want:** after an app restart, a table the user already loaded paints from
cache immediately (stale-while-revalidate), instead of an empty grid until the
API answers — painful on throttled backends (lldev 504s take ~50s to fail).

**Seam exists:** `vantage-diorama/src/lens/mod.rs` defines `CacheBackend`;
`memory_cache.rs` is the only impl ("one IndexMap per Dio, no persistence").

**Shape:** a disk-backed `CacheBackend` (sqlite or CBOR file per Dio cache
key), keyed the same way sceneries dedup (conditions/sort/search must be part
of the key or the sorted variant misses). Load on open → rows paint instantly
→ live fetches overwrite. Eviction/expiry policy TBD (age cap is probably
enough for a viewer).

**Consumer:** vantage-ui framework pages (and legacy grids) get restart-warm
grids for free once the backend wires a persistent cache into `Dio` creation.
