# Changelog

## 0.1.4 — 2026-08-09

- `DioRouter` gains `with_id_map`, `with_record_projection` and
  `with_not_found_message`, so a route can match an existing API contract
  exactly: rename the id field, replace the record body with a projection, and
  choose the 404 text.
- `axum_action::ActionRouter` mounts a model's `ModelAction` set as POST
  routes with no per-action code: `POST /{table}/{id}/actions/{name}` for a
  record action, `POST /{table}/actions/{name}` for a table action. `alias`
  hands out a single handler for a legacy path. An alias path names its own
  captures, so a single capture is taken as the record id whatever it is
  called (`/tag/{tag_id}/register`); `id` only has to be spelled out when the
  path captures more than one value.
- Actions are keyed by table and name together, so two models exposing the
  same action name both keep their routes; a genuine duplicate is an error at
  construction rather than a dropped route.
- The response status comes from the error's `ErrorKind` — `NotFound` is 404,
  `IncorrectUsage` is 400, anything else is a logged 500. Previously the
  status was guessed from the message text, which answered 4xx for database
  outages and told clients to retry with different input.

## 0.1.3 — 2026-07-27

- Depend on vantage-diorama 0.9 (client-side search and the augmentation spec
  layer removed; no adapter-side behavior change).

## 0.1.2 — 2026-07-22

- Depend on vantage-diorama 0.7 (WriteOp retired for ChangeFlash; no
  adapter-side behavior change).

## 0.1.1 — 2026-07-16

- `DioRouter::key_by` now accepts non-string ids. The identity-watch diff keyed a
  row only when its id field serialized to a JSON string, silently skipping every
  row otherwise — so a watch keyed on a SurrealDB `Thing` (a tagged object) or a
  numeric id emitted nothing. It now derives a stable key from any JSON value, so
  `key_by` works across backends (string, numeric, Mongo `ObjectId`, Surreal
  `Thing`). String ids are unchanged.
