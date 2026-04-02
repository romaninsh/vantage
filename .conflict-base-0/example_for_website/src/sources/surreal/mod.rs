// TODO: SurrealDB source
//
// Will provide a SurrealDB-backed ReadableValueSet using vantage-surrealdb.
//
// Example usage (once implemented):
//
//   let db = SurrealSource::connect("ws://root:root@localhost:8000/bakery/v2").await?;
//   let table = db.table("users");
//   export_jsonl(&table, |v| v).await?
