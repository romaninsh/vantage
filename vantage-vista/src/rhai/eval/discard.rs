//! The compile-time check for statements that build a table handle and drop
//! it.
//!
//! A narrowing verb returns a new handle and leaves the one it was called on
//! unchanged, so a statement like `self.where("a", 1);` followed by another
//! statement builds a handle nobody reads: its narrowing is lost. Such a
//! script is refused when it compiles. Chain the calls instead
//! (`self.where("a", 1).sort("n")`) or assign the handle
//! (`let paid = self.where("status", "paid");`).
//!
//! One rule covers every script: outside value position, a statement whose
//! call chain ends on a narrowing verb is refused, whatever the chain is
//! called on. A script whose value is a table handle (`modify:`, reference
//! build-scripts) has a value position: its
//! last statement, and inside an `if`/`else` or block in that position, their
//! own last statements. A trailing `;` doesn't change which statement that
//! is. A statement script, whose value is ignored (a faker sim), has none.

use vantage_core::{Result, error};
use vantage_rhai::rhai::{AST, Expr, FnPtr, Stmt};

/// The verbs that return a narrowed copy of the handle they are called on,
/// the backend extension `with_condition` included.
const NARROWING_VERBS: [&str; 7] = [
    "where",
    "sort",
    "search",
    "limit",
    "offset",
    "ref",
    "with_condition",
];

/// Refuse `ast` when a statement outside value position ends on a narrowing
/// verb (`where`, `sort`, `search`, `limit`, `offset`, `ref`, `with_condition`) and so
/// drops the handle it returns, as in `let open = table();
/// open.where("status", "open");`. `has_value` says whether the script's
/// value is used: when it is, the statement giving the script its value may
/// end on a narrowing verb.
///
/// Terminals and writes (`table().insert(..);`) are statements as usual, and
/// so is rhai's in-place array sort (`ids.sort();`, `ids.sort(|a, b| ..);`):
/// `sort` counts as narrowing only with one or two arguments, the first not
/// a function pointer or closure. A comparer held in a variable
/// (`ids.sort(cmp);`) can't be told from a column name, so it is refused;
/// pass the closure inline.
///
/// The check covers the script's top-level statements and the bodies of
/// `if`/`else`, `try`/`catch`, blocks and loops. It doesn't descend into
/// `fn` definitions or `switch` arms. `what` names the script in the error.
pub fn reject_dropped_narrowing(ast: &AST, what: &'static str, has_value: bool) -> Result<()> {
    check_block(ast.statements(), has_value, what)
}

fn check_block(statements: &[Stmt], in_value_position: bool, what: &'static str) -> Result<()> {
    let last = statements.len().saturating_sub(1);
    for (i, statement) in statements.iter().enumerate() {
        check_statement(statement, in_value_position && i == last, what)?;
    }
    Ok(())
}

fn check_statement(statement: &Stmt, in_value_position: bool, what: &'static str) -> Result<()> {
    match statement {
        Stmt::Expr(expr) => match expr.as_ref() {
            Expr::Stmt(block) => check_block(block.statements(), in_value_position, what),
            expr if !in_value_position && ends_on_narrowing(expr) => Err(error!(
                "this statement's table handle is discarded; chain the calls or assign it",
                script = what,
                line = statement.position().line().unwrap_or(0)
            )
            .mark_incorrect_usage()),
            _ => Ok(()),
        },
        Stmt::Block(block) => check_block(block.statements(), in_value_position, what),
        Stmt::If(flow, _) => {
            check_block(flow.body.statements(), in_value_position, what)?;
            check_block(flow.branch.statements(), in_value_position, what)
        }
        // rhai's optimizer drops a try body's result, so no statement in it, or
        // in its catch, is in value position.
        Stmt::TryCatch(flow, _) => {
            check_block(flow.body.statements(), false, what)?;
            check_block(flow.branch.statements(), false, what)
        }
        // A loop's value is `()`, so no statement in its body is in value position.
        Stmt::While(flow, _) | Stmt::Do(flow, ..) => {
            check_block(flow.body.statements(), false, what)
        }
        Stmt::For(parts, _) => check_block(parts.2.body.statements(), false, what),
        _ => Ok(()),
    }
}

/// A `.`-chain whose last call is a narrowing verb.
fn ends_on_narrowing(expr: &Expr) -> bool {
    let mut last = expr;
    while let Expr::Dot(chain, ..) = last {
        last = &chain.rhs;
    }
    if std::ptr::eq(last, expr) {
        return false;
    }
    let Expr::MethodCall(call, _) = last else {
        return false;
    };
    match call.name.as_str() {
        // rhai's own in-place array sort, `a.sort()` or `a.sort(|x, y| ..)`,
        // returns `()`. A handle's sort takes a column and an optional
        // direction.
        "sort" => {
            matches!(call.args.len(), 1 | 2) && !call.args.first().is_some_and(is_function_value)
        }
        name => NARROWING_VERBS.contains(&name),
    }
}

/// A function pointer or closure literal: `Fn("name")`, `|x| ..`, or a
/// closure that captures variables (a block sharing them, ending on a
/// `curry` call).
fn is_function_value(expr: &Expr) -> bool {
    match expr {
        Expr::DynamicConstant(value, _) => value.is::<FnPtr>(),
        Expr::FnCall(call, _) => is_fn_constructor(call.name.as_str()),
        Expr::Stmt(block) => match block.statements().last() {
            Some(Stmt::FnCall(call, _)) => is_fn_constructor(call.name.as_str()),
            Some(Stmt::Expr(expr)) => is_function_value(expr),
            _ => false,
        },
        _ => false,
    }
}

fn is_fn_constructor(name: &str) -> bool {
    matches!(name, "Fn" | "curry")
}
