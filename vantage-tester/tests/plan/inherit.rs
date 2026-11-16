use vantage_tester::{Branch, Do, Node, Session, Tree};

use super::plan_yaml;

const BRANCHING: &str = r#"
scenarios:
  main:
    steps:
      - step: close
        clock: +2d
        before: |
          set(acme, #{ funded: true })
        vars: { closed_loan: { record: loan, from: result.loan } }
        as: desktop
        do: { action: close }
        assert: [closed]
        stub: { mailer: { ok: true } }
        what_if: [quiet, refused, bare]
  bare:
    stub: { sms: { ok: true } }
    steps:
      - step: close bare
  quiet:
    steps:
      - step: close quietly
        before: 'set(x, #{})'
      - step: then more
  refused:
    steps:
      - step: close is refused
        fails: "nope"
"#;

fn branch(tree: &Tree, i: usize) -> &Node {
    match &tree.roots[0].steps[0].what_ifs[i] {
        Branch::Scenario(node) => node,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_first_step_inherits_do_as_and_assert() {
    let tree = plan_yaml(BRANCHING).unwrap();
    let quiet = branch(&tree, 0);
    let first = &quiet.steps[0].step;
    assert_eq!(first.session, Some(Session::Desktop));
    assert!(matches!(&first.do_, Some(Do::Action { action, .. }) if action == "close"));
    assert_eq!(first.assert, ["closed"]);
    assert_eq!(first.before.as_deref(), Some("set(x, #{})"));
    let second = &quiet.steps[1].step;
    assert!(second.do_.is_none() && second.assert.is_empty() && second.session.is_none());
    assert!(second.clock.is_none() && second.before.is_none() && second.stub.is_empty());

    let refused = &branch(&tree, 1).steps[0].step;
    assert!(refused.do_.is_some());
    assert!(
        refused.assert.is_empty(),
        "fails drops the inherited assert"
    );
    assert_eq!(refused.fails.as_deref(), Some("nope"));
}

#[test]
fn a_first_step_inherits_clock_before_and_stub() {
    let tree = plan_yaml(BRANCHING).unwrap();
    let bare = branch(&tree, 2);
    let first = &bare.steps[0].step;
    assert_eq!(first.step, "close bare");
    assert_eq!(first.clock.map(|c| c.to_string()).as_deref(), Some("+2d"));
    assert_eq!(
        first.before.as_deref(),
        Some("set(acme, #{ funded: true })\n")
    );
    let stubs: Vec<_> = first.stub.keys().map(String::as_str).collect();
    assert_eq!(stubs, ["sms", "mailer"]);
    assert!(first.vars.is_empty(), "vars are never inherited");
    assert!(
        bare.steps[0].what_ifs.is_empty(),
        "what_if is never inherited"
    );
}
