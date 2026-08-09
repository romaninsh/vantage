# Changelog

## 0.6.0 — 2026-08-09

Initial release. A model defines a business operation once and every
transport reuses it: an HTTP layer mounts it as a route, a UI renders a
form from its input schema, a script calls it as a host function.

- `ModelAction` — the transport-facing contract: a descriptor plus an
  async `invoke(target, args)`.
- `ActionDescriptor` carries the name, the `ActionTarget` (a whole table
  or one record), a description, and JSON Schemas for input and output.
- `record_action` / `table_action` wrap a typed async closure as a
  `dyn ModelAction`, handling deserialization, serialization, and the
  `<table>:` prefix on record keys.
- `actions` — the attribute macro, re-exported from the nested
  `vantage-action-macros` crate. Put it on a trait and it generates one
  constructor per `#[action]` method from the signature: `&self` is the
  target, `#[id]` is the record key a transport fills in, the owned
  parameter is the input, and remaining `&T` parameters become `Arc<T>`
  constructor arguments.
- Input that fails to deserialize, and a target that does not match the
  descriptor, are marked `ErrorKind::IncorrectUsage`, so a transport can
  answer 4xx without reading the message text.
