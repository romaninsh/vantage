use std::collections::HashMap;

use serde_json::json;

use surreal_client::SessionState;

#[test]
fn new_session_is_empty_and_unauthenticated() {
    let session = SessionState::new();

    assert_eq!(session.get_target(), (None, None));
    assert!(session.token.is_none());
    assert!(session.scope.is_none());
    assert!(session.params().is_empty());
    assert!(!session.is_authenticated());
}

#[test]
fn setters_update_target_token_and_scope() {
    let mut session = SessionState::new();
    session.set_namespace(Some("ns".into()));
    session.set_database(Some("db".into()));
    session.set_token(Some("jwt".into()));
    session.set_scope(Some("users".into()));

    assert_eq!(
        session.get_target(),
        (Some(&"ns".to_string()), Some(&"db".to_string()))
    );
    assert_eq!(session.token.as_deref(), Some("jwt"));
    assert_eq!(session.scope.as_deref(), Some("users"));
}

#[test]
fn set_target_sets_namespace_and_database_together() {
    let mut session = SessionState::new();
    session.set_target(Some("ns".into()), Some("db".into()));

    assert_eq!(session.namespace.as_deref(), Some("ns"));
    assert_eq!(session.database.as_deref(), Some("db"));
}

#[test]
fn a_token_makes_the_session_authenticated() {
    let mut session = SessionState::new();
    session.set_token(Some("jwt".into()));
    assert!(session.is_authenticated());

    session.set_token(None);
    assert!(!session.is_authenticated());
}

#[test]
fn params_can_be_set_read_and_unset() {
    let mut session = SessionState::new();
    session.set_param("user_id".into(), json!(123));
    session.set_param("role".into(), json!("admin"));

    assert_eq!(session.get_param("user_id"), Some(&json!(123)));
    assert_eq!(session.params().len(), 2);

    session.unset_param("user_id");
    assert_eq!(session.get_param("user_id"), None);
    assert_eq!(session.get_param("role"), Some(&json!("admin")));
}

#[test]
fn clear_params_removes_every_param() {
    let mut session = SessionState::new();
    session.set_param("a".into(), json!(1));
    session.set_param("b".into(), json!(2));

    session.clear_params();
    assert!(session.params().is_empty());
}

#[test]
fn merge_params_adds_new_keys_and_overwrites_existing() {
    let mut session = SessionState::new();
    session.set_param("existing".into(), json!("old"));
    session.set_param("kept".into(), json!("same"));

    let mut incoming = HashMap::new();
    incoming.insert("existing".to_string(), json!("new"));
    incoming.insert("added".to_string(), json!(true));
    session.merge_params(incoming);

    assert_eq!(session.get_param("existing"), Some(&json!("new")));
    assert_eq!(session.get_param("added"), Some(&json!(true)));
    assert_eq!(session.get_param("kept"), Some(&json!("same")));
}

#[test]
fn clear_auth_drops_token_and_scope_only() {
    let mut session = SessionState::new();
    session.set_target(Some("ns".into()), Some("db".into()));
    session.set_token(Some("jwt".into()));
    session.set_scope(Some("users".into()));
    session.set_param("a".into(), json!(1));

    session.clear_auth();

    assert!(session.token.is_none());
    assert!(session.scope.is_none());
    assert!(!session.is_authenticated());
    assert_eq!(session.namespace.as_deref(), Some("ns"));
    assert_eq!(session.get_param("a"), Some(&json!(1)));
}

#[test]
fn reset_clears_the_whole_session() {
    let mut session = SessionState::new();
    session.set_target(Some("ns".into()), Some("db".into()));
    session.set_token(Some("jwt".into()));
    session.set_scope(Some("users".into()));
    session.set_param("a".into(), json!(1));

    session.reset();

    assert_eq!(session.get_target(), (None, None));
    assert!(session.token.is_none());
    assert!(session.scope.is_none());
    assert!(session.params().is_empty());
}
