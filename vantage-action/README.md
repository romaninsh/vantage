# vantage-action

Model-level actions for [Vantage](https://github.com/romaninsh/vantage): typed
business operations that a model crate defines once and every transport
reuses. An action is a serializable input, a serializable output and an async
Rust body. HTTP mounts it as a route, a UI builds a form from its input
schema, and a Rhai host can register it as a function.

```toml
[dependencies]
vantage-action = "0.7.0"
```

## `#[actions]`

Put `#[actions]` above `#[async_trait]` on a trait, mark methods with
`#[action(name, table)]`, and it generates one constructor per action:

```rust,ignore
use vantage_action::{Target, actions};

#[actions]
#[async_trait]
trait Greeter {
    /// Greet one record.
    #[action(name = "greet_one", table = "person")]
    async fn greet_one(&self, #[id] id: &str, req: Greeting, rec: &Recorder) -> Result<Greeted>;

    /// Greet the whole table.
    #[action(name = "greet_all", table = "person")]
    async fn greet_all(&self, req: Greeting) -> Result<Greeted>;
}

let one = greet_one_action(Store, Arc::new(Recorder::default()));
let out = one
    .invoke(Target::Id("person:ab".into()), serde_json::json!({ "who": "sam" }))
    .await?;

let all = greet_all_action(Store);
all.invoke(Target::None, serde_json::json!({ "who": "sam" })).await?;
```

Parameters are sorted by role:

- `&self` is the target the constructor captures (a `Table`, or a mock in
  tests).
- `#[id] name: &str` makes it a record action. Without it, it is a table
  action.
- Exactly one owned parameter is the input. It must be `Deserialize +
  JsonSchema`.
- Other `&T` parameters are dependencies, passed to the constructor as
  `Arc<T>`.

The doc comment becomes the action's description. The attribute comes from
`vantage-action-macros`; depend on `vantage-action` and use
`vantage_action::actions`.

## Main types

- `ModelAction`: `descriptor()` and `invoke(Target, serde_json::Value)`,
  returning an `ActionOutput` (`Value` or `Record`).
- `ActionDescriptor`: `name`, `target` (`ActionTarget::Table` or
  `ActionTarget::Record`), `description`, and `input_schema` /
  `output_schema` as JSON Schema.
- `record_action(..)` and `table_action(..)` wrap a typed async closure as an
  `Arc<dyn ModelAction>`. The macro calls these; use them directly for
  signatures the macro can't express.
- Record keys may arrive as `GH123` or `tag:GH123`; `bare_key` strips the
  table prefix, so bodies always see `GH123`. Bad input is marked
  `IncorrectUsage`, which HTTP transports answer with a 4xx.

## Related crates

`vantage-api-adapters` mounts actions as `POST` routes. `vantage-bundle`
lets a compiled model crate hand its actions to an application.
