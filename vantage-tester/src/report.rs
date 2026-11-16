mod junit;

use indexmap::IndexMap;
use serde::Serialize;
use serde_json::Value;

use crate::tree::Unwritten;

/// green < yellow < red; a set of results takes the worst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Green,
    Yellow,
    Red,
}

/// A run over one or more files.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub files: Vec<FileReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileReport {
    pub path: String,
    pub status: Status,
    /// The host's `load` failed; nothing ran.
    pub error: Option<String>,
    /// In walk order: a scenario, then the what-ifs that branch from it.
    pub scenarios: Vec<ScenarioReport>,
    pub unwritten: Vec<Unwritten>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioReport {
    pub scenario: String,
    /// The scenarios it branched from, nearest first. Empty for a root.
    pub from: Vec<String>,
    pub skip: Option<String>,
    /// The host couldn't start it (`root` or a what-if's `fork` failed); its steps didn't run.
    pub error: Option<String>,
    pub steps: Vec<StepReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepReport {
    pub step: String,
    /// Who the step ran as ([`crate::Session::label`]); `None` when neither the step nor its
    /// scenario names a session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub who: Option<String>,
    pub outcome: Outcome,
    pub result: Value,
    pub writes: Vec<Write>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    /// `assert` is the expression that was false or raised, when one did.
    Failed {
        message: String,
        assert: Option<String>,
    },
    /// An earlier step failed, the step this what-if branches from failed, or the scenario
    /// didn't start.
    NotRun {
        reason: String,
    },
}

/// One row a step wrote, found by comparing the store before and after.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Write {
    pub table: String,
    pub op: WriteOp,
    pub id: String,
    /// New values of the changed columns; empty for a delete.
    pub columns: IndexMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WriteOp {
    Insert,
    Update,
    Delete,
}

impl Report {
    pub fn status(&self) -> Status {
        self.files
            .iter()
            .map(|f| f.status)
            .max()
            .unwrap_or(Status::Green)
    }

    pub fn to_junit(&self) -> String {
        junit::render(self)
    }
}

impl FileReport {
    pub(crate) fn settle(&mut self) {
        let failed = self.error.is_some() || self.scenarios.iter().any(ScenarioReport::failed);
        self.status = if failed {
            Status::Red
        } else if !self.unwritten.is_empty() {
            Status::Yellow
        } else {
            Status::Green
        };
    }
}

impl ScenarioReport {
    pub fn label(&self) -> String {
        crate::tree::label(&self.scenario, &self.from)
    }

    pub fn failed(&self) -> bool {
        self.error.is_some()
            || self
                .steps
                .iter()
                .any(|s| matches!(s.outcome, Outcome::Failed { .. }))
    }
}
