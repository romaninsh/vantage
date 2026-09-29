//! Config shape of [`ColumnGen`]: a map with exactly one key naming the
//! generator. Spelled out as a struct of optional variants rather than an
//! externally tagged enum, because YAML deserializers disagree on enums (some
//! demand `!tag` syntax); a plain map reads the same in every format.

use serde::Deserialize;

use super::{ColumnGen, Spread};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Wire {
    pick: Option<Pick>,
    range: Option<Range>,
    date: Option<Date>,
    sentence: Option<Sentence>,
    pattern: Option<String>,
    walk: Option<Walk>,
    tree: Option<Tree>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pick {
    values: Vec<String>,
    #[serde(default)]
    weights: Option<Vec<f64>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Range {
    min: f64,
    max: f64,
    #[serde(default)]
    decimals: Option<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Date {
    from: String,
    #[serde(default = "default_to")]
    to: String,
    #[serde(default)]
    spread: Spread,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sentence {
    #[serde(default = "default_min_words")]
    min_words: u8,
    #[serde(default = "default_max_words")]
    max_words: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Walk {
    start: f64,
    step: f64,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    decimals: Option<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tree {
    roots: usize,
    depth: u8,
    #[serde(default)]
    min_depth: Option<u8>,
}

fn default_to() -> String {
    "now".to_string()
}
fn default_min_words() -> u8 {
    4
}
fn default_max_words() -> u8 {
    10
}

impl TryFrom<Wire> for ColumnGen {
    type Error = String;

    fn try_from(w: Wire) -> Result<Self, String> {
        let mut found = Vec::new();
        if let Some(Pick { values, weights }) = w.pick {
            found.push(ColumnGen::Pick { values, weights });
        }
        if let Some(Range { min, max, decimals }) = w.range {
            found.push(ColumnGen::Range { min, max, decimals });
        }
        if let Some(Date { from, to, spread }) = w.date {
            found.push(ColumnGen::Date { from, to, spread });
        }
        if let Some(Sentence {
            min_words,
            max_words,
        }) = w.sentence
        {
            found.push(ColumnGen::Sentence {
                min_words,
                max_words,
            });
        }
        if let Some(template) = w.pattern {
            found.push(ColumnGen::Pattern(template));
        }
        if let Some(Walk {
            start,
            step,
            min,
            max,
            decimals,
        }) = w.walk
        {
            found.push(ColumnGen::Walk {
                start,
                step,
                min,
                max,
                decimals,
            });
        }
        if let Some(Tree {
            roots,
            depth,
            min_depth,
        }) = w.tree
        {
            found.push(ColumnGen::Tree {
                roots,
                depth,
                min_depth,
            });
        }
        match found.len() {
            1 => Ok(found.remove(0)),
            0 => Err("expected one of pick, range, date, sentence, pattern, walk, tree".into()),
            _ => Err("a column takes exactly one generator".into()),
        }
    }
}
