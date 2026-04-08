use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::mysql::types::AnyMysqlType;

type Expr = Expression<AnyMysqlType>;

/// A column definition within a JSON_TABLE expression.
///
/// # Examples
///
/// ```ignore
/// JsonTableColumn::new("voltage", "INT", "$.specs.voltage")
///     .default("0")
///     .on_empty()
/// ```
#[derive(Debug, Clone)]
pub struct JsonTableColumn {
    name: String,
    col_type: String,
    path: String,
    default: Option<String>,
    on_empty: bool,
    on_error: bool,
}

impl JsonTableColumn {
    pub fn new(
        name: impl Into<String>,
        col_type: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            col_type: col_type.into(),
            path: path.into(),
            default: None,
            on_empty: false,
            on_error: false,
        }
    }

    pub fn default(mut self, value: impl Into<String>) -> Self {
        self.default = Some(value.into());
        self
    }

    pub fn on_empty(mut self) -> Self {
        self.on_empty = true;
        self
    }

    pub fn on_error(mut self) -> Self {
        self.on_error = true;
        self
    }

    fn render(&self) -> String {
        let mut s = format!("{} {} PATH '{}'", self.name, self.col_type, self.path);
        if let Some(ref def) = self.default {
            s.push_str(&format!(" DEFAULT '{}' ", def.replace('\'', "''")));
            if self.on_empty {
                s.push_str("ON EMPTY");
            }
            if self.on_error {
                if self.on_empty {
                    s.push(' ');
                }
                s.push_str("ON ERROR");
            }
        }
        s
    }
}

/// MySQL JSON_TABLE — turn JSON into relational rows.
///
/// Renders as:
/// ```sql
/// JSON_TABLE(source, '$' COLUMNS (
///     col1 TYPE PATH '$.path1',
///     col2 TYPE PATH '$.path2'
/// ))
/// ```
///
/// # Examples
///
/// ```ignore
/// JsonTable::new(ident("metadata").dot_of("p"))
///     .column(JsonTableColumn::new("voltage", "INT", "$.specs.voltage").default("0").on_empty())
///     .column(JsonTableColumn::new("watts", "INT", "$.specs.watts").default("0").on_empty())
/// ```
#[derive(Debug, Clone)]
pub struct JsonTable {
    source: Expr,
    root_path: String,
    columns: Vec<JsonTableColumn>,
}

impl JsonTable {
    pub fn new(source: impl Expressive<AnyMysqlType>) -> Self {
        Self {
            source: source.expr(),
            root_path: "$".to_string(),
            columns: Vec::new(),
        }
    }

    pub fn root_path(mut self, path: impl Into<String>) -> Self {
        self.root_path = path.into();
        self
    }

    pub fn column(mut self, col: JsonTableColumn) -> Self {
        self.columns.push(col);
        self
    }
}

impl Expressive<AnyMysqlType> for JsonTable {
    fn expr(&self) -> Expr {
        let columns_sql = self
            .columns
            .iter()
            .map(|c| c.render())
            .collect::<Vec<_>>()
            .join(", ");

        Expression::new(
            format!(
                "JSON_TABLE({{}}, '{}' COLUMNS ({}))",
                self.root_path, columns_sql
            ),
            vec![ExpressiveEnum::Nested(self.source.clone())],
        )
    }
}
