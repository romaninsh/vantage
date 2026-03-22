// TODO: REST API source
//
// Will provide an HTTP-backed ReadableValueSet using reqwest.
// Expects the endpoint to return a JSON array of objects.
//
// Example usage (once implemented):
//
//   let source = ApiSource::new("https://api.example.com/users");
//   export_jsonl(&source, |v| v).await?
