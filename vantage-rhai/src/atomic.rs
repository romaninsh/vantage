//! The grammar of `atomic { BODY }` and `atomic { BODY } rollback (NAME) { ARM }`,
//! defined once: a host that runs blocks registers it with its own `eval`,
//! a validator with one that never runs.

use rhai::{Dynamic, Engine, EvalAltResult, EvalContext, Expression, ImmutableString, ParseError};

/// Registers the `atomic` syntax on `engine`. `eval` gets the block's
/// expression, then, with a rollback arm, the arm's `NAME` and its block.
pub fn register_atomic_syntax(
    engine: &mut Engine,
    eval: impl Fn(&mut EvalContext, &[Expression]) -> Result<Dynamic, Box<EvalAltResult>>
    + Send
    + Sync
    + 'static,
) {
    // The arm pushes NAME, so the parser must look later names up by name
    // rather than by a slot it counted.
    engine.register_custom_syntax_with_state_raw(
        "atomic",
        parse_atomic,
        true,
        move |context, inputs, _| eval(context, inputs),
    );
}

fn parse_atomic(
    symbols: &[ImmutableString],
    look_ahead: &str,
    _state: &mut Dynamic,
) -> Result<Option<ImmutableString>, ParseError> {
    let next = match symbols.len() {
        1 => "$block$",
        2 if look_ahead == "rollback" => "rollback",
        3 => "(",
        4 => "$ident$",
        5 => ")",
        6 => "$block$",
        _ => return Ok(None),
    };
    Ok(Some(next.into()))
}
