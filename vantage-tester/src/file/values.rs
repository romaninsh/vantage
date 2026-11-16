use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::{Map, Value};
use vantage_core::{Result, VantageError, error};

/// A named handle on the data.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "RawVar")]
pub enum VarDef {
    /// `{ record: <table>, id: <id> }` or, in a step, `{ record: <table>, from: <rhai> }`.
    Record {
        record: String,
        source: RecordSource,
    },
    /// `{ vista: <table>, where: { … } }`.
    Vista {
        vista: String,
        where_: IndexMap<String, Value>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecordSource {
    Id(String),
    /// A Rhai expression over `result`.
    From(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVar {
    record: Option<String>,
    id: Option<Value>,
    from: Option<String>,
    vista: Option<String>,
    #[serde(rename = "where")]
    where_: Option<IndexMap<String, Value>>,
}

impl TryFrom<RawVar> for VarDef {
    type Error = VantageError;

    fn try_from(raw: RawVar) -> Result<Self> {
        match raw {
            RawVar {
                record: Some(record),
                vista: None,
                where_: None,
                id,
                from,
            } => {
                let source = match (id, from) {
                    (Some(id), None) => RecordSource::Id(scalar_id(id)?),
                    (None, Some(from)) => RecordSource::From(from),
                    _ => {
                        return Err(error!(
                            "a record variable needs exactly one of `id` or `from`"
                        ));
                    }
                };
                Ok(VarDef::Record { record, source })
            }
            RawVar {
                vista: Some(vista),
                record: None,
                id: None,
                from: None,
                where_,
            } => Ok(VarDef::Vista {
                vista,
                where_: where_.unwrap_or_default(),
            }),
            _ => Err(error!(
                "a variable is `{ record: <table>, id | from }` or `{ vista: <table>, where: {…} }`"
            )),
        }
    }
}

fn scalar_id(v: Value) -> Result<String> {
    match v {
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(error!("a record id is a string or a number", got = other)),
    }
}

/// Who a step runs as.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "Value")]
pub enum Session {
    Desktop,
    /// `{ app: <api>, <field>: … }`; field values may name variables (`$carol`).
    Api {
        app: String,
        fields: Map<String, Value>,
    },
}

impl Session {
    /// Who the step runs as, for a progress line: `desktop`, or the session's field values
    /// by field name and the API's name (`Marta · manager · south @staff` for `name`, `role`,
    /// `team_id`). A variable (`$carol`) shows as written.
    pub fn label(&self) -> String {
        match self {
            Session::Desktop => "desktop".into(),
            Session::Api { app, fields } => {
                let values: Vec<String> = fields
                    .values()
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .collect();
                if values.is_empty() {
                    format!("@{app}")
                } else {
                    format!("{} @{app}", values.join(" · "))
                }
            }
        }
    }
}

impl TryFrom<Value> for Session {
    type Error = VantageError;

    fn try_from(v: Value) -> Result<Self> {
        match v {
            Value::String(s) if s == "desktop" => Ok(Session::Desktop),
            Value::Object(mut fields) => match fields.remove("app") {
                Some(Value::String(app)) => Ok(Session::Api { app, fields }),
                _ => Err(error!("an API session needs `app: <api name>`")),
            },
            other => Err(error!(
                "a session is `desktop` or `{ app: <api>, … }`",
                got = other
            )),
        }
    }
}

/// The behaviour under test.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "Value")]
pub enum Do {
    /// `{ action: <name>, args: { … }, record: $var }`.
    Action {
        action: String,
        args: Option<Value>,
        record: Option<String>,
    },
    /// A Rhai block that runs as an action body would.
    Script(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAction {
    action: String,
    args: Option<Value>,
    record: Option<String>,
}

impl TryFrom<Value> for Do {
    type Error = VantageError;

    fn try_from(v: Value) -> Result<Self> {
        match v {
            Value::String(s) => Ok(Do::Script(s)),
            v @ Value::Object(_) => {
                let raw: RawAction =
                    serde_json::from_value(v).map_err(|e| error!(format!("`do`: {e}")))?;
                Ok(Do::Action {
                    action: raw.action,
                    args: raw.args,
                    record: raw.record,
                })
            }
            other => Err(error!(
                "`do` is `{ action: … }` or a Rhai block",
                got = other
            )),
        }
    }
}

/// A step's `what_if:` list of outcome scenarios, or `true` when they aren't named yet.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "Value")]
pub enum WhatIf {
    Named(Vec<String>),
    Unnamed,
}

impl TryFrom<Value> for WhatIf {
    type Error = VantageError;

    fn try_from(v: Value) -> Result<Self> {
        let names = match v {
            Value::Bool(true) => return Ok(WhatIf::Unnamed),
            Value::String(s) => vec![Value::String(s)],
            Value::Array(items) => items,
            other => {
                return Err(error!(
                    "`what_if` is a list of scenario names or `true`",
                    got = other
                ));
            }
        };
        names
            .into_iter()
            .map(|n| match n {
                Value::String(s) => Ok(s),
                other => Err(error!("a `what_if` entry is a scenario name", got = other)),
            })
            .collect::<Result<_>>()
            .map(WhatIf::Named)
    }
}
