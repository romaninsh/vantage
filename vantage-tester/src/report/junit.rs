use std::fmt::Write as _;

use super::{FileReport, Outcome, Report, ScenarioReport};

enum Kind {
    Passed,
    Failed(String),
    Skipped(String),
}

struct Case {
    class: String,
    name: String,
    kind: Kind,
}

/// One `<testsuite>` per file, one `<testcase>` per step (classname = scenario label). A
/// skipped scenario is one skipped case, a scenario that couldn't start one failed `(start)`
/// case, an unwritten what-if one skipped case, and a file that didn't load one failed
/// `(load)` case.
pub(super) fn render(report: &Report) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites>\n");
    for file in &report.files {
        let cases = file_cases(file);
        let failures = cases
            .iter()
            .filter(|c| matches!(c.kind, Kind::Failed(_)))
            .count();
        let skipped = cases
            .iter()
            .filter(|c| matches!(c.kind, Kind::Skipped(_)))
            .count();
        let _ = writeln!(
            out,
            "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{failures}\" skipped=\"{skipped}\">",
            esc(&file.path),
            cases.len()
        );
        for case in &cases {
            let body = match &case.kind {
                Kind::Passed => String::new(),
                Kind::Failed(message) => {
                    let first = message.lines().next().unwrap_or_default();
                    format!(
                        "<failure message=\"{}\">{}</failure>",
                        esc(first),
                        esc(message)
                    )
                }
                Kind::Skipped(reason) => format!("<skipped message=\"{}\"/>", esc(reason)),
            };
            let _ = writeln!(
                out,
                "    <testcase classname=\"{}\" name=\"{}\">{body}</testcase>",
                esc(&case.class),
                esc(&case.name)
            );
        }
        out.push_str("  </testsuite>\n");
    }
    out.push_str("</testsuites>\n");
    out
}

fn file_cases(file: &FileReport) -> Vec<Case> {
    if let Some(error) = &file.error {
        return vec![case(&file.path, "(load)", Kind::Failed(error.clone()))];
    }
    let mut cases: Vec<Case> = file.scenarios.iter().flat_map(scenario_cases).collect();
    for u in &file.unwritten {
        let name = match &u.name {
            Some(name) => format!("what-if {name} ({})", u.step),
            None => format!("what_if: true ({})", u.step),
        };
        cases.push(case(&u.from, &name, Kind::Skipped("not written".into())));
    }
    cases
}

fn scenario_cases(scenario: &ScenarioReport) -> Vec<Case> {
    let class = scenario.label();
    if let Some(reason) = &scenario.skip {
        return vec![case(&class, "(skipped)", Kind::Skipped(reason.clone()))];
    }
    if let Some(error) = &scenario.error {
        return vec![case(&class, "(start)", Kind::Failed(error.clone()))];
    }
    scenario
        .steps
        .iter()
        .map(|step| {
            let kind = match &step.outcome {
                Outcome::Passed => Kind::Passed,
                Outcome::Failed {
                    message,
                    assert: Some(a),
                } => Kind::Failed(format!("{message}\nassert: {a}")),
                Outcome::Failed {
                    message,
                    assert: None,
                } => Kind::Failed(message.clone()),
                Outcome::NotRun { reason } => Kind::Skipped(reason.clone()),
            };
            case(&class, &step.step, kind)
        })
        .collect()
}

fn case(class: &str, name: &str, kind: Kind) -> Case {
    Case {
        class: class.to_string(),
        name: name.to_string(),
        kind,
    }
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}
