# Code Review Findings — vantage

Generated 2026-06-10 by an automated multi-agent review. Each file contains a snippet, severity, and recommendation.

**Total: 59** (security: 11 · bugs: 18 · inconsistencies: 14 · omissions: 11 · performance: 3 · suggestions: 2)

## Security (11)

| Severity | Finding | Location |
|---|---|---|
| critical | [SurrealQL injection via Thing/record-id rendered into query text](security/data-surreal-thing-injection.md) | `vantage-surrealdb/src/thing.rs:132` |
| high | [DebugEngine prints all RPC params and responses (incl. secrets) to stdout](security/data-surreal-debug-logging.md) | `surreal-client/src/engines/debug.rs:21` |
| high | [SurrealDB Identifier escaping is incomplete and bypassable](security/data-surreal-identifier-escaping.md) | `vantage-surrealdb/src/identifier.rs:49` |
| high | [SurrealQL injection in inline single-quoted primitives (similarity / time_group)](security/data-surreal-inline-string-injection.md) | `vantage-surrealdb/src/primitives.rs:219` |
| high | [API auth tokens exposed via derived Debug on RestApi/GraphqlApi](security/infra-auth-header-in-debug.md) | `vantage-api-client/src/rest/api.rs:103` |
| medium | [search_table_condition embeds parameter placeholder inside a quoted SQL literal](security/core-search-condition-placeholder-in-literal.md) | `vantage-table/src/mocks/mock_table_source.rs:159-168` |
| medium | [MySQL JSON_TABLE inlines name/type/path without escaping](security/data-mysql-jsontable-escaping.md) | `vantage-sql/src/mysql/statements/primitives/json_table.rs:57` |
| medium | [vantage-sql Identifier does not escape embedded quote characters](security/data-sql-identifier-quote-escaping.md) | `vantage-sql/src/primitives/identifier.rs:67` |
| medium | [Plaintext credentials exposed through derived Debug on connection/auth types](security/data-surreal-credentials-in-debug-derive.md) | `surreal-client/src/connection.rs:31` |
| medium | [Cmd/CmdSpec Debug output includes declared env (often credentials)](security/infra-cmd-debug-env-secrets.md) | `vantage-cmd/src/cmd.rs:80` |
| low | [LogWriter table name joins into the path unchecked (traversal out of base_dir)](security/infra-log-writer-path-traversal.md) | `vantage-log-writer/src/log_writer.rs:56` |

## Bugs (18)

| Severity | Finding | Location |
|---|---|---|
| high | [ImDataSource read-modify-write race loses concurrent updates](bugs/core-im-datasource-lost-update-race.md) | `vantage-dataset/src/im/mod.rs:50-58` |
| high | [LICENSE carries someone else's copyright; Apache-2.0 file missing despite dual-license claim](bugs/docs-license-wrong-copyright.md) | `LICENSE:3` |
| high | [Failed pool requests are dropped, leaving callers awaiting forever](bugs/infra-pool-error-request-hangs-caller.md) | `vantage-api-pool/src/client_pool/http.rs:101` |
| medium | [IntoValue for f64 panics on NaN/Infinity](bugs/core-f64-into-value-panic.md) | `vantage-expressions/src/value.rs:31` |
| medium | [IntoRecord blanket impls panic on entity serialization failure](bugs/core-into-record-expect-panic.md) | `vantage-types/src/record.rs:237` |
| medium | [get_table_count_expr calls Handle::block_on inside the runtime — guaranteed panic](bugs/core-mock-count-block-on-panic.md) | `vantage-table/src/mocks/mock_table_source.rs:492-520` |
| medium | [Table::select_column unwraps column lookup — panics on unknown field name](bugs/core-select-column-unwrap.md) | `vantage-table/src/table/impls/selectable.rs:135` |
| medium | [CSV search filter is silently a no-op (returns all rows)](bugs/data-csv-search-noop.md) | `vantage-csv/src/table_source.rs:65` |
| medium | [bakery_model3 README documents a `cli` example that doesn't exist and outdated sources/version](bugs/docs-bakery3-readme-cli-example.md) | `bakery_model3/README.md:43` |
| medium | [README references crates and an example file that don't exist (vantage-config, bakery_api, bakery_model/examples)](bugs/docs-readme-nonexistent-crates-and-example.md) | `README.md:84` |
| medium | [Unbounded retries and uncapped server-controlled Retry-After sleep](bugs/infra-pool-unbounded-retries.md) | `vantage-api-pool/src/eventual_request/mod.rs:117` |
| medium | [RestApi get_table_value/get_table_count only see the current page](bugs/infra-rest-get-value-page-scoped.md) | `vantage-api-client/src/rest/table_source.rs:131` |
| medium | [RestApi search condition is silently dropped — search returns all rows](bugs/infra-rest-search-silent-noop.md) | `vantage-api-client/src/rest/table_source.rs:103` |
| low | [Expression::preview() corrupts output when parameter values contain `{}`](bugs/core-preview-replacen-corruption.md) | `vantage-expressions/src/expression/core.rs:168-175` |
| low | [SurrealDB get_table_value ignores the table's id field name](bugs/data-surreal-get-value-id-field.md) | `vantage-surrealdb/src/surrealdb/impls/table_source.rs:216` |
| low | [Broken links: book step8 link, vantage-table → expressions README, rustdoc-only link in crates.io README](bugs/docs-broken-doc-links.md) | `docs4/src/new-persistence.md:121` |
| low | [README code snippets contain Rust syntax errors and typos](bugs/docs-readme-snippet-syntax-errors.md) | `README.md:35` |
| low | [HttpClientPool::with_rate_limit silently does nothing when pool was built without one](bugs/infra-rate-limit-silent-noop.md) | `vantage-api-pool/src/client_pool/http.rs:109` |

## Inconsistencies (14)

| Severity | Finding | Location |
|---|---|---|
| high | [insert/delete idempotency contracts contradicted by the framework's own implementations](inconsistencies/core-insert-delete-idempotency-contract.md) | `vantage-dataset/src/traits/dataset.rs:179-189` |
| high | [vantage-config.schema.json is the old-prototype schema; doesn't match what code parses](inconsistencies/docs-config-schema-stale.md) | `vantage-config.schema.json:1` |
| high | [README documents type-erasure API (AnyTable, AnyDataSet) that does not exist in code](inconsistencies/docs-readme-anytable-anydataset-fiction.md) | `README.md:99` |
| high | [README DataSet/Pagination examples use method names that don't exist](inconsistencies/docs-readme-dataset-api-names.md) | `README.md:241-283` |
| high | [README and book claim "0.4" while all crates are 0.5.x; install snippet pins "0.4"](inconsistencies/docs-version-04-vs-05-crates.md) | `README.md:90` |
| medium | [ActiveEntity::save() replaces, ActiveRecord::save() patches — same API name, different semantics](inconsistencies/core-active-save-semantics-diverge.md) | `vantage-dataset/src/record.rs:32-34` |
| medium | [`search_table_condition` has wildly different semantics per engine](inconsistencies/data-search-semantics-divergence.md) | `vantage-sql/src/sqlite/impls/table_source.rs:110` |
| medium | [SQL backends disagree on placeholder/param-count validation](inconsistencies/data-sql-placeholder-validation-divergence.md) | `vantage-sql/src/sqlite/impls/expr_data_source.rs:89` |
| medium | [README says PostgreSQL/MySQL are "coming next" but vantage-sql already implements them](inconsistencies/docs-readme-postgres-mysql-status.md) | `README.md:665` |
| medium | [README flips Table generic parameter order and shows outdated with_many closure shape](inconsistencies/docs-readme-table-generics-flip.md) | `README.md:293-297` |
| medium | ["vantage-ui-adapters" is actually package `dataset-ui-adapters`, unpublished, with unusable Quick Start](inconsistencies/docs-ui-adapters-crate-name.md) | `vantage-ui-adapters/README.md:22` |
| medium | [AWS_ENDPOINT_URL override honoured only by json1/json10 transports](inconsistencies/infra-aws-endpoint-override-ignored.md) | `vantage-aws/src/restjson/transport.rs:30` |
| low | [Mixed id-parameter conventions across dataset traits, and trait docs showing outdated signatures](inconsistencies/core-id-param-conventions-and-doc-drift.md) | `vantage-dataset/src/traits/dataset.rs:125,189` |
| low | [README backend-status table step numbers don't match the Persistence Guide it cites](inconsistencies/docs-readme-status-table-step-numbers.md) | `README.md:709-716` |

## Omissions (11)

| Severity | Finding | Location |
|---|---|---|
| medium | [get_count_via_query returns Ok(0) for unrecognized result shapes](omissions/core-count-query-silent-zero.md) | `vantage-table/src/table/impls/selectable.rs:209-221` |
| medium | [Stale source files outside the module tree (would not compile if re-enabled)](omissions/core-dead-source-files.md) | `vantage-table/src/with_columns.rs:1` |
| medium | [Flatten::resolve_deferred is a documented no-op — flatten() silently skips deferred parameters](omissions/core-resolve-deferred-noop.md) | `vantage-expressions/src/expression/flatten.rs:65-70` |
| medium | [redb panics (not Err) on unsupported search — reachable from generic code](omissions/data-redb-csv-panic-on-unsupported.md) | `vantage-redb/src/redb/impls/table_source.rs:72` |
| medium | [Root README omits Vista, Diorama and other shipped crates that the book headlines](omissions/docs-readme-missing-vista-diorama.md) | `README.md:64-88` |
| medium | [Root CHANGELOG.md ends at 0.2.0 (2025-02-16) — three releases behind](omissions/docs-root-changelog-stale.md) | `CHANGELOG.md:5` |
| medium | [AwwPool caches the first auth token forever — no expiry or refresh](omissions/infra-awwpool-token-never-refreshed.md) | `vantage-api-pool/src/aww_pool.rs:110` |
| medium | [vantage-cmd subprocesses have no timeout and unbounded output capture](omissions/infra-cmd-run-no-timeout.md) | `vantage-cmd/src/exec.rs:66` |
| medium | [No timeouts on any HTTP client (RestApi, GraphqlApi, AwsAccount, pool workers)](omissions/infra-no-http-timeouts.md) | `vantage-api-client/src/rest/api.rs:536` |
| medium | [unimplemented!() panics reachable through normal TableSource API](omissions/infra-unimplemented-panics-reachable.md) | `vantage-api-pool/src/pool_api.rs:371` |
| low | [MockTableSource::with_data silently drops rows without "id" — count and list disagree](omissions/core-mock-with-data-drops-idless-rows.md) | `vantage-table/src/mocks/mock_table_source.rs:50-77` |

## Performance (3)

| Severity | Finding | Location |
|---|---|---|
| medium | [Every ImTable operation clones the entire table](performance/core-im-table-full-clone-per-op.md) | `vantage-dataset/src/im/mod.rs:50-53` |
| low | [CSV backend re-reads and re-parses the whole file on every operation](performance/data-csv-full-file-rescan.md) | `vantage-csv/src/table_source.rs:93` |
| low | [AwwPool::get builds a fresh reqwest::Client per call](performance/infra-awwpool-new-client-per-get.md) | `vantage-api-pool/src/aww_pool.rs:75` |

## Suggestions (2)

| Severity | Finding | Location |
|---|---|---|
| low | [VantageError `is_*` methods are builders, not predicates](suggestions/core-errorkind-is-naming.md) | `vantage-core/src/util/error.rs:236-256` |
| low | [Unify the two divergent SurrealDB identifier-escaping implementations](suggestions/data-unify-surreal-identifier-escaping.md) | `vantage-surrealdb/src/identifier.rs:49` |
