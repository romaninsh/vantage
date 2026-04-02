// TODO: PostgreSQL source
//
// Will provide a PostgreSQL-backed ReadableValueSet using vantage-postgres (pending crate).
//
// Example usage (once implemented):
//
//   let db = PostgresSource::connect("postgres://user:pass@localhost/mydb").await?;
//   let table = db.table("users");
//   export_jsonl(&table, |v| v).await?
