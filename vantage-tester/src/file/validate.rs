mod cut;

use std::collections::HashSet;

use indexmap::IndexMap;
use vantage_core::{Result, error};

use super::{Do, RecordSource, ScenarioFile, Step, VarDef, WhatIf};
use cut::check_not_cut;

/// Rhai keywords and the names the host binds itself.
const RESERVED: &[&str] = &[
    "as", "break", "catch", "const", "continue", "do", "else", "export", "false", "fn", "for",
    "global", "if", "import", "in", "let", "loop", "private", "result", "return", "row", "switch",
    "this", "throw", "true", "try", "until", "while",
];

/// Checks that need only the file: names, keys that can't go together, references.
pub(crate) fn validate(file: &ScenarioFile) -> Result<()> {
    check_vars(&file.vars, "file `vars`", None)?;
    for (name, var) in &file.vars {
        if let VarDef::Record {
            source: RecordSource::From(_),
            ..
        } = var
        {
            return Err(error!(
                "a file-level variable can't use `from`; declare it in a step's `vars`",
                var = name
            ));
        }
    }
    let named = named_scenarios(file);
    for (name, scenario) in &file.scenarios {
        if let Some(clock) = &scenario.clock
            && clock.trim_start().starts_with('+')
        {
            return Err(error!(
                "a scenario's `clock` is an absolute timestamp; offsets go on steps",
                scenario = name
            ));
        }
        if let Some(base) = &scenario.extends
            && !file.scenarios.contains_key(base)
        {
            return Err(error!(
                "`extends` names no scenario in this file",
                scenario = name,
                extends = base
            ));
        }
        if named.contains(name.as_str()) {
            for (key, set) in [
                ("session", scenario.session.is_some()),
                ("clock", scenario.clock.is_some()),
                ("extends", scenario.extends.is_some()),
            ] {
                if set {
                    return Err(error!(
                        format!(
                            "scenario `{name}` is a what-if and starts where it branches; it can't set `{key}`"
                        ),
                        scenario = name
                    ));
                }
            }
        }
        check_steps(name, &scenario.steps, &file.vars)?;
    }
    Ok(())
}

/// Every scenario some step names in its `what_if`.
pub(crate) fn named_scenarios(file: &ScenarioFile) -> HashSet<&str> {
    file.scenarios
        .values()
        .flat_map(|s| &s.steps)
        .filter_map(|step| match &step.what_if {
            Some(WhatIf::Named(names)) => Some(names),
            _ => None,
        })
        .flatten()
        .map(String::as_str)
        .collect()
}

pub(crate) fn check_unique_sentences<'a>(
    scenario: &str,
    sentences: impl IntoIterator<Item = &'a str>,
) -> Result<()> {
    let mut seen = HashSet::new();
    for sentence in sentences {
        if !seen.insert(sentence) {
            return Err(error!(
                format!(
                    "step `{sentence}` appears twice; a step's sentence must be unique in its scenario"
                ),
                scenario = scenario
            ));
        }
    }
    Ok(())
}

fn check_steps(scenario: &str, steps: &[Step], file_vars: &IndexMap<String, VarDef>) -> Result<()> {
    for step in steps {
        if step.step.trim().is_empty() {
            return Err(error!(
                "a step needs a sentence in `step`",
                scenario = scenario
            ));
        }
        if step.fails.is_some() && !step.vars.is_empty() {
            return Err(error!(
                "a `fails:` step has no result to declare `vars` from",
                scenario = scenario,
                step = step.step
            ));
        }
        if step.fails.as_deref().is_some_and(|f| f.trim().is_empty()) {
            return Err(error!(
                "`fails:` needs the text the failure must contain",
                scenario = scenario,
                step = step.step
            ));
        }
        let place = format!("`vars` of step `{}`", step.step);
        check_vars(&step.vars, &place, Some(scenario))?;
        if let Some(shadow) = step.vars.keys().find(|v| file_vars.contains_key(*v)) {
            return Err(error!(
                format!("step variable `{shadow}` shadows the file-level variable of that name"),
                scenario = scenario,
                step = step.step
            ));
        }
        let script = match &step.do_ {
            Some(Do::Script(s)) => Some(s),
            _ => None,
        };
        let from = step.vars.values().filter_map(|v| match v {
            VarDef::Record {
                source: RecordSource::From(f),
                ..
            } => Some(f),
            _ => None,
        });
        let assert = step.assert.iter().flat_map(|a| a.iter());
        for rhai in step.before.iter().chain(script).chain(assert).chain(from) {
            check_not_cut(scenario, &step.step, rhai)?;
        }
    }
    check_unique_sentences(scenario, steps.iter().map(|s| s.step.as_str()))
}

fn check_vars(vars: &IndexMap<String, VarDef>, place: &str, scenario: Option<&str>) -> Result<()> {
    for name in vars.keys() {
        if !is_identifier(name) || RESERVED.contains(&name.as_str()) {
            let mut err = error!(format!(
                "variable `{name}` in {place} must be a Rhai identifier and not a keyword"
            ));
            if let Some(scenario) = scenario {
                err.context
                    .insert("scenario".into(), format!("{scenario:?}"));
            }
            return Err(err);
        }
    }
    Ok(())
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.chars().any(|c| c != '_')
}
