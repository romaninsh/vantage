use std::collections::HashSet;

use indexmap::IndexMap;
use vantage_core::{Result, error};

use super::{Branch, Node, PlannedStep, ResolvedStep, Tree};
use crate::file::{Scenario, ScenarioFile, Session, Step, Stubs, WhatIf, validate};

pub(super) fn plan(file: &ScenarioFile) -> Result<Tree> {
    let mut resolved = IndexMap::new();
    for name in file.scenarios.keys() {
        resolve(file, name, &mut Vec::new(), &mut resolved)?;
    }
    let named = validate::named_scenarios(file);
    let mut builder = Builder {
        resolved: &resolved,
        reached: HashSet::new(),
    };
    let mut roots = Vec::new();
    for (name, scenario) in &resolved {
        if named.contains(name.as_str()) {
            continue;
        }
        let at = Branching {
            session: scenario.session.clone(),
            stub: Stubs::new(),
            anchor: None,
        };
        roots.push(builder.node(name, &at, &[])?);
    }
    if let Some(lost) = named
        .iter()
        .find(|n| resolved.contains_key(**n) && !builder.reached.contains(**n))
    {
        return Err(error!(
            "what-if cycle: no root scenario leads to this scenario",
            scenario = lost
        ));
    }
    Ok(Tree { roots })
}

/// Applies `extends`: settings named here override the base's, stubs merge, steps append.
/// `skip` belongs to the scenario itself and is not inherited.
fn resolve(
    file: &ScenarioFile,
    name: &str,
    stack: &mut Vec<String>,
    done: &mut IndexMap<String, Scenario>,
) -> Result<Scenario> {
    if let Some(s) = done.get(name) {
        return Ok(s.clone());
    }
    if stack.iter().any(|s| s == name) {
        stack.push(name.to_string());
        return Err(error!(
            format!("`extends` cycle: {}", stack.join(" → ")),
            scenario = name
        ));
    }
    let own = &file.scenarios[name];
    let merged = match &own.extends {
        None => own.clone(),
        Some(base_name) => {
            stack.push(name.to_string());
            let base = resolve(file, base_name, stack, done)?;
            stack.pop();
            let mut stub = base.stub;
            stub.extend(own.stub.clone());
            let mut steps = base.steps;
            steps.extend(own.steps.iter().cloned());
            validate::check_unique_sentences(name, steps.iter().map(|s| s.step.as_str()))?;
            Scenario {
                session: own.session.clone().or(base.session),
                clock: own.clock.clone().or(base.clock),
                extends: own.extends.clone(),
                stub,
                skip: own.skip.clone(),
                steps,
            }
        }
    };
    done.insert(name.to_string(), merged.clone());
    Ok(merged)
}

/// Where a node starts: the session and stubs of the scenario it branches from, and the step
/// its first step replaces (with that step's own inheritance applied).
struct Branching {
    session: Option<Session>,
    stub: Stubs,
    anchor: Option<Step>,
}

struct Builder<'a> {
    resolved: &'a IndexMap<String, Scenario>,
    reached: HashSet<String>,
}

impl Builder<'_> {
    fn node(&mut self, name: &str, at: &Branching, chain: &[String]) -> Result<Node> {
        if chain.iter().any(|c| c == name) {
            return Err(error!(format!(
                "what-if cycle: `{name}` branches from itself"
            )));
        }
        self.reached.insert(name.to_string());
        let scenario = self.resolved[name].clone();
        let mut node = Node {
            name: name.to_string(),
            scenario,
            steps: Vec::new(),
        };
        if node.scenario.skip.is_some() {
            self.mark_targets(name);
            return Ok(node);
        }
        if at.anchor.is_some() && node.scenario.steps.is_empty() {
            return Err(error!(
                "a what-if scenario needs steps, or `skip:` with a reason",
                scenario = name
            ));
        }
        let mut stub = at.stub.clone();
        stub.extend(node.scenario.stub.clone());
        let mut inner = chain.to_vec();
        inner.push(name.to_string());
        for (i, step) in node.scenario.steps.iter().enumerate() {
            let step = match (i, &at.anchor) {
                (0, Some(anchor)) => inherit(step, anchor),
                _ => step.clone(),
            };
            let resolved = resolve_step(&step, &at.session, &stub);
            if resolved.fails.is_some() && resolved.do_.is_none() {
                return Err(error!(
                    "a `fails:` step needs a `do` to fail",
                    scenario = name,
                    step = step.step
                ));
            }
            let mut what_ifs = Vec::new();
            match &step.what_if {
                None => {}
                Some(WhatIf::Unnamed) => what_ifs.push(Branch::Unnamed),
                Some(WhatIf::Named(names)) => {
                    for target in names {
                        if !self.resolved.contains_key(target) {
                            what_ifs.push(Branch::Missing(target.clone()));
                            continue;
                        }
                        let from = Branching {
                            session: at.session.clone(),
                            stub: stub.clone(),
                            anchor: Some(step.clone()),
                        };
                        let node = self.node(target, &from, &inner)?;
                        what_ifs.push(Branch::Scenario(Box::new(node)));
                    }
                }
            }
            node.steps.push(PlannedStep {
                step: resolved,
                what_ifs,
            });
        }
        Ok(node)
    }

    /// A skipped node builds no branches, but the scenarios its steps name are still reached
    /// through it, so they aren't taken for a what-if cycle.
    fn mark_targets(&mut self, name: &str) {
        let targets: Vec<String> = self.resolved[name]
            .steps
            .iter()
            .filter_map(|s| match &s.what_if {
                Some(WhatIf::Named(names)) => Some(names.clone()),
                _ => None,
            })
            .flatten()
            .filter(|t| self.resolved.contains_key(t))
            .collect();
        for target in targets {
            if self.reached.insert(target.clone()) {
                self.mark_targets(&target);
            }
        }
    }
}

/// A what-if's first step takes everything it leaves out (`clock`, `as`, `before`, `do`, `stub`,
/// `assert`) from the step it replaces, never `step`, `vars` or `what_if`; with `fails:` it
/// doesn't take `assert`.
fn inherit(step: &Step, anchor: &Step) -> Step {
    let assert = match &step.assert {
        None if step.fails.is_none() => anchor.assert.clone(),
        own => own.clone(),
    };
    Step {
        clock: step.clock.or(anchor.clock),
        as_: step.as_.clone().or_else(|| anchor.as_.clone()),
        before: step.before.clone().or_else(|| anchor.before.clone()),
        do_: step.do_.clone().or_else(|| anchor.do_.clone()),
        stub: if step.stub.is_empty() {
            anchor.stub.clone()
        } else {
            step.stub.clone()
        },
        assert,
        ..step.clone()
    }
}

fn resolve_step(step: &Step, session: &Option<Session>, stub: &Stubs) -> ResolvedStep {
    let mut merged = stub.clone();
    merged.extend(step.stub.clone());
    ResolvedStep {
        step: step.step.clone(),
        clock: step.clock,
        session: step.as_.clone().or_else(|| session.clone()),
        before: step.before.clone(),
        do_: step.do_.clone(),
        assert: step
            .assert
            .as_ref()
            .map(|a| a.0.clone())
            .unwrap_or_default(),
        fails: step.fails.clone(),
        vars: step.vars.clone(),
        stub: merged,
    }
}
