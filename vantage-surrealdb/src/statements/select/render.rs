use crate::{AnySurrealType, Expr, surreal_expr};
use vantage_expressions::result::QueryResult;
use vantage_expressions::{Expression, Expressive};

use super::SurrealSelect;
use super::select_target::SelectTarget;

impl<T: QueryResult> SurrealSelect<T> {
    /// Renders the SELECT fields clause
    fn render_fields(&self) -> Expr {
        if self.fields.is_empty() {
            surreal_expr!("*")
        } else {
            let field_expressions: Vec<Expr> = self
                .fields
                .iter()
                .map(|field| field.clone().into())
                .collect();
            Expression::from_vec(field_expressions, ", ")
        }
    }

    /// Renders the FROM clause
    fn render_from(&self) -> Expr {
        if self.from.is_empty() {
            surreal_expr!("")
        } else {
            let from_expressions: Vec<Expr> = self
                .from
                .iter()
                .map(|target: &SelectTarget| target.clone().into())
                .collect();
            surreal_expr!(
                format!(" FROM {}{{}}", if self.from_only { "ONLY " } else { "" }),
                (Expression::from_vec(from_expressions, ", "))
            )
        }
    }

    /// Renders the WHERE clause
    fn render_where(&self) -> Expr {
        if self.where_conditions.is_empty() {
            surreal_expr!("")
        } else {
            let combined = and_all(self.where_conditions.clone());
            surreal_expr!(" WHERE {}", (combined))
        }
    }

    /// Renders the SPLIT clause (after WHERE, before GROUP).
    fn render_split(&self) -> Expr {
        if self.split.is_empty() {
            surreal_expr!("")
        } else {
            let split_expressions: Vec<Expr> = self.split.to_vec();
            surreal_expr!(" SPLIT {}", (Expression::from_vec(split_expressions, ", ")))
        }
    }

    /// Renders the GROUP clause. `GROUP ALL` is mutually exclusive with
    /// `GROUP BY` and takes precedence when set.
    fn render_group(&self) -> Expr {
        if self.group_all {
            surreal_expr!(" GROUP ALL")
        } else if self.group_by.is_empty() {
            surreal_expr!("")
        } else {
            let group_expressions: Vec<Expr> = self.group_by.to_vec();
            surreal_expr!(
                " GROUP BY {}",
                (Expression::from_vec(group_expressions, ", "))
            )
        }
    }

    /// Renders the ORDER BY clause
    fn render_order_by(&self) -> Expr {
        if self.order_by.is_empty() {
            surreal_expr!("")
        } else {
            let order_expressions: Vec<Expr> = self
                .order_by
                .iter()
                .map(|(expression, ascending)| {
                    if *ascending {
                        surreal_expr!("{}", (expression.clone()))
                    } else {
                        surreal_expr!("{} DESC", (expression.clone()))
                    }
                })
                .collect();
            let combined = Expression::from_vec(order_expressions, ", ");
            surreal_expr!(" ORDER BY {}", (combined))
        }
    }

    /// Renders the LIMIT and START clauses
    fn render_limit(&self) -> Expr {
        match (self.limit, self.skip) {
            (Some(limit), Some(skip)) => surreal_expr!(" LIMIT {} START {}", limit, skip),
            (Some(limit), None) => surreal_expr!(" LIMIT {}", limit),
            (None, Some(skip)) => surreal_expr!(" START {}", skip),
            (None, None) => surreal_expr!(""),
        }
    }

    /// Renders entire statement into an expression
    pub(crate) fn render(&self) -> Expr {
        surreal_expr!(
            "SELECT {}{}{}{}{}{}{}{}",
            (if self.single_value {
                surreal_expr!("VALUE ")
            } else {
                surreal_expr!("")
            }),
            (self.render_fields()),
            (self.render_from()),
            (self.render_where()),
            (self.render_split()),
            (self.render_group()),
            (self.render_order_by()),
            (self.render_limit())
        )
    }

    /// The statement as text, values inlined. For debugging only: never
    /// embed it in another query.
    pub fn preview(&self) -> String {
        self.render().preview()
    }
}

impl<T: QueryResult> Expressive<AnySurrealType> for SurrealSelect<T> {
    fn expr(&self) -> Expr {
        self.render()
    }
}

/// `conditions` joined with `AND`. Next to another condition, one holding an
/// operator that binds looser than `AND` is wrapped, so
/// `x = $parent.id AND a OR b` can't read `b` rows of every parent.
/// Conditions without one render bare, as before.
pub(crate) fn and_all(conditions: Vec<Expr>) -> Expr {
    let many = conditions.len() > 1;
    let parts = conditions
        .into_iter()
        .map(|c| {
            if many && binds_looser_than_and(&c.preview()) {
                surreal_expr!("({})", (c))
            } else {
                c
            }
        })
        .collect();
    Expression::from_vec(parts, " AND ")
}

/// Whether `surql` holds `OR` as a word (any spacing, any case), `||`, or a
/// null-coalescing `??` / `?:`, outside one pair of parentheses around the
/// whole condition. It errs towards `true`: a match inside a literal or a
/// sub-select only adds parentheses.
fn binds_looser_than_and(surql: &str) -> bool {
    if enclosed(surql) {
        return false;
    }
    let upper = surql.to_ascii_uppercase();
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    let or_word = upper.match_indices("OR").any(|(at, _)| {
        !upper[..at].chars().next_back().is_some_and(word)
            && !upper[at + 2..].chars().next().is_some_and(word)
    });
    or_word || ["||", "??", "?:"].iter().any(|op| upper.contains(op))
}

/// Whether `text` is one parenthesised group: its first `(` closes at its
/// last character. A `(` or `)` inside a literal can only make this `false`,
/// which wraps once more.
fn enclosed(text: &str) -> bool {
    let text = text.trim();
    if !text.starts_with('(') {
        return false;
    }
    let mut depth = 0usize;
    for (at, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return at + 1 == text.len();
                }
            }
            _ => {}
        }
    }
    false
}
