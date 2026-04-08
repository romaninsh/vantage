use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::mysql::types::AnyMysqlType;

type Expr = Expression<AnyMysqlType>;

/// Search mode for MATCH ... AGAINST.
#[derive(Debug, Clone)]
pub enum FulltextMode {
    /// No explicit mode (MySQL defaults to natural language).
    Default,
    /// IN NATURAL LANGUAGE MODE
    NaturalLanguage,
    /// IN BOOLEAN MODE
    Boolean,
    /// WITH QUERY EXPANSION
    QueryExpansion,
}

/// MySQL MATCH(...) AGAINST('...' [mode]) fulltext search expression.
///
/// # Examples
///
/// ```ignore
/// FulltextMatch::new([
///     ident("name").dot_of("p"),
///     ident("description").dot_of("p"),
/// ])
/// .against("pro features")
/// .natural_language_mode()
/// ```
#[derive(Debug, Clone)]
pub struct FulltextMatch {
    columns: Vec<Expr>,
    query: String,
    mode: FulltextMode,
}

impl FulltextMatch {
    pub fn new(columns: impl IntoIterator<Item = impl Expressive<AnyMysqlType>>) -> Self {
        Self {
            columns: columns.into_iter().map(|c| c.expr()).collect(),
            query: String::new(),
            mode: FulltextMode::Default,
        }
    }

    pub fn against(mut self, query: impl Into<String>) -> Self {
        self.query = query.into();
        self
    }

    pub fn natural_language_mode(mut self) -> Self {
        self.mode = FulltextMode::NaturalLanguage;
        self
    }

    pub fn boolean_mode(mut self) -> Self {
        self.mode = FulltextMode::Boolean;
        self
    }

    pub fn with_query_expansion(mut self) -> Self {
        self.mode = FulltextMode::QueryExpansion;
        self
    }
}

impl Expressive<AnyMysqlType> for FulltextMatch {
    fn expr(&self) -> Expr {
        let cols = Expression::from_vec(self.columns.clone(), ", ");
        let mode_suffix = match self.mode {
            FulltextMode::Default => "",
            FulltextMode::NaturalLanguage => " IN NATURAL LANGUAGE MODE",
            FulltextMode::Boolean => " IN BOOLEAN MODE",
            FulltextMode::QueryExpansion => " WITH QUERY EXPANSION",
        };

        Expression::new(
            format!("MATCH({{}}) AGAINST({{}}{mode_suffix})"),
            vec![
                ExpressiveEnum::Nested(cols),
                ExpressiveEnum::Scalar(AnyMysqlType::from(self.query.clone())),
            ],
        )
    }
}
