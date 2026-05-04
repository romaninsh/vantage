# Changelog

## 0.4.1 — 2026-05-04

- New error-kind annotators: [`VantageError::is_unimplemented`](https://docs.rs/vantage-core/0.4.1/vantage_core/util/error/struct.VantageError.html) and [`is_unsupported`](https://docs.rs/vantage-core/0.4.1/vantage_core/util/error/struct.VantageError.html). Use them on errors returned from default trait method impls so callers can distinguish "the driver doesn't support this op" from "the driver claims support but the implementation is missing."

## 0.4.0 — 2026-04-16

- Initial 0.4 release. `VantageError` with structured context, `Result` alias, and the unified error-handling foundation used by every other vantage crate.
