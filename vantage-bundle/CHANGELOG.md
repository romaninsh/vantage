# Changelog

## 0.6.0 — 2026-08-09

Initial release. The contract a compiled-in model crate implements to
serve its tables and actions to an application.

These traits already existed inside the Vantage UI application. Living
there, they could only be implemented by code inside that application,
so shipping a customer's model meant carrying a branch of the
application itself: a Cargo feature, an optional path dependency to the
customer's repository, and a hand-written shim, all re-merged on every
release. Published here, a model crate implements the traits directly
and any application can compile it in.

- `ModelBundle` — the tables a bundle provides, the `Vista` behind each
  one, and the operations that go with them.
- `BundleTable` — a table's catalog key, its datasource, and a title.
  No schema: the application introspects the real shape from the Vista,
  so the model crate cannot drift from a description of itself.
- `BuiltinAction` — the older untyped action body, JSON in and JSON out,
  for screens that declare their own fields.
