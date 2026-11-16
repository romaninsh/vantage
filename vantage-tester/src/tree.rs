mod build;

use indexmap::IndexMap;
use serde::Serialize;
use vantage_core::Result;

use crate::file::{ClockOffset, Do, Scenario, ScenarioFile, Session, Stubs, VarDef, validate};

/// The what-if tree of one file: its root scenarios, each step carrying the outcomes that
/// branch from it.
#[derive(Debug, Clone)]
pub struct Tree {
    pub roots: Vec<Node>,
}

/// A scenario as it runs at one place in the tree. A scenario named by two steps appears as
/// two nodes.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    /// The scenario after `extends`: settings merged, steps appended. A what-if node's
    /// settings come from where it branches, so only a root's are handed to the host.
    pub scenario: Scenario,
    pub steps: Vec<PlannedStep>,
}

#[derive(Debug, Clone)]
pub struct PlannedStep {
    pub step: ResolvedStep,
    pub what_ifs: Vec<Branch>,
}

#[derive(Debug, Clone)]
pub enum Branch {
    Scenario(Box<Node>),
    /// Named in `what_if` but not written. Keeps the file yellow.
    Missing(String),
    /// `what_if: true`. Keeps the file yellow.
    Unnamed,
}

/// A step ready to run: first-step inheritance applied, the session and stubs it runs with
/// worked out.
#[derive(Debug, Clone, Default)]
pub struct ResolvedStep {
    pub step: String,
    pub clock: Option<ClockOffset>,
    /// The session the step runs as: its `as` (for a what-if's first step, possibly inherited),
    /// else its scenario's.
    pub session: Option<Session>,
    pub before: Option<String>,
    pub do_: Option<Do>,
    pub assert: Vec<String>,
    pub fails: Option<String>,
    pub vars: IndexMap<String, VarDef>,
    /// The scenario's stubs with the step's merged over them.
    pub stub: Stubs,
}

/// An outcome the file names but nobody wrote: a missing scenario or a `what_if: true`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unwritten {
    /// `None` for `what_if: true`.
    pub name: Option<String>,
    /// Label of the scenario whose step names it (`funded`, `revoked ← funded`).
    pub from: String,
    pub step: String,
}

/// Resolves `extends`, anchors every what-if at the step naming it and applies first-step
/// inheritance. Refuses cycles, duplicate sentences and what-ifs that set their own start.
pub fn plan(file: &ScenarioFile) -> Result<Tree> {
    validate::validate(file)
        .and_then(|()| build::plan(file))
        .map_err(|mut e| {
            e.context.insert("file".into(), format!("{:?}", file.path));
            e
        })
}

impl Tree {
    /// Steps in the whole tree, each node counted once, skipped scenarios and what they
    /// branch to left out: the total a walk reports on.
    pub fn step_count(&self) -> usize {
        fn count(node: &Node) -> usize {
            if node.scenario.skip.is_some() {
                return 0;
            }
            node.steps
                .iter()
                .map(|p| {
                    let below: usize = p
                        .what_ifs
                        .iter()
                        .map(|b| match b {
                            Branch::Scenario(n) => count(n),
                            _ => 0,
                        })
                        .sum();
                    1 + below
                })
                .sum()
        }
        self.roots.iter().map(count).sum()
    }

    pub fn unwritten(&self) -> Vec<Unwritten> {
        let mut out = Vec::new();
        for root in &self.roots {
            root.collect_unwritten(&[], &mut out);
        }
        out
    }
}

impl Node {
    pub(crate) fn collect_unwritten(&self, chain: &[String], out: &mut Vec<Unwritten>) {
        if self.scenario.skip.is_some() {
            return;
        }
        let label = label(&self.name, chain);
        let mut inner = chain.to_vec();
        inner.insert(0, self.name.clone());
        for planned in &self.steps {
            for branch in &planned.what_ifs {
                let name = match branch {
                    Branch::Scenario(node) => {
                        node.collect_unwritten(&inner, out);
                        continue;
                    }
                    Branch::Missing(name) => Some(name.clone()),
                    Branch::Unnamed => None,
                };
                out.push(Unwritten {
                    name,
                    from: label.clone(),
                    step: planned.step.step.clone(),
                });
            }
        }
    }
}

/// `revoked ← funded`: the scenario, then the scenarios it branched from, nearest first.
pub(crate) fn label(name: &str, chain: &[String]) -> String {
    std::iter::once(name)
        .chain(chain.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ← ")
}
