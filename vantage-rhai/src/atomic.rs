//! The grammar of `atomic { BODY }` and `atomic { BODY } rollback (NAME[, NAME]) { ARM }`,
//! defined once: a host that runs blocks registers it with its own `eval`,
//! a validator with one that never runs.

use rhai::{Dynamic, Engine, EvalAltResult, EvalContext, Expression, ImmutableString, ParseError};

/// Registers the `atomic` syntax on `engine`. `eval` gets the block's
/// expression, then, with a rollback arm, the arm's one or two `NAME`s and
/// its block: `[body]`, `[body, name, arm]` or `[body, name, name, arm]`.
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
    let two_names = symbols.get(5).is_some_and(|s| s.as_str() == ",");
    let next = match symbols.len() {
        1 => "$block$",
        2 if look_ahead == "rollback" => "rollback",
        3 => "(",
        4 => "$ident$",
        5 if look_ahead == "," => ",",
        5 => ")",
        6 if two_names => "$ident$",
        6 => "$block$",
        7 if two_names => ")",
        8 if two_names => "$block$",
        _ => return Ok(None),
    };
    Ok(Some(next.into()))
}
