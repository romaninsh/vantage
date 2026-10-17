use serde_json::json;

use surreal_client::params::{ConnectParams, SigninParams};

#[test]
fn connect_params_builder_sets_each_field() {
    let params = ConnectParams::new()
        .with_namespace("ns")
        .with_database("db")
        .with_version_check(false);

    assert_eq!(params.namespace.as_deref(), Some("ns"));
    assert_eq!(params.database.as_deref(), Some("db"));
    assert_eq!(params.version_check, Some(false));
}

#[test]
fn connect_params_start_empty() {
    let params = ConnectParams::new();
    assert!(params.namespace.is_none());
    assert!(params.database.is_none());
    assert!(params.version_check.is_none());
}

#[test]
fn signin_root_sends_only_user_and_pass() {
    let rpc = SigninParams::root("admin", "secret").to_rpc_params();

    assert_eq!(rpc.len(), 2);
    assert_eq!(rpc.get("user"), Some(&json!("admin")));
    assert_eq!(rpc.get("pass"), Some(&json!("secret")));
}

#[test]
fn signin_namespace_adds_ns() {
    let rpc = SigninParams::namespace("u", "p", "my_ns").to_rpc_params();

    assert_eq!(rpc.len(), 3);
    assert_eq!(rpc.get("user"), Some(&json!("u")));
    assert_eq!(rpc.get("pass"), Some(&json!("p")));
    assert_eq!(rpc.get("NS"), Some(&json!("my_ns")));
}

#[test]
fn signin_database_adds_ns_and_db() {
    let rpc = SigninParams::database("u", "p", "my_ns", "my_db").to_rpc_params();

    assert_eq!(rpc.len(), 4);
    assert_eq!(rpc.get("NS"), Some(&json!("my_ns")));
    assert_eq!(rpc.get("DB"), Some(&json!("my_db")));
}

#[test]
fn signin_scope_uses_sc_abbreviation() {
    let rpc = SigninParams::scope("my_ns", "my_db", "user_scope").to_rpc_params();

    assert_eq!(rpc.len(), 3);
    assert_eq!(rpc.get("NS"), Some(&json!("my_ns")));
    assert_eq!(rpc.get("DB"), Some(&json!("my_db")));
    assert_eq!(rpc.get("SC"), Some(&json!("user_scope")));
}

#[test]
fn signin_access_uses_ac_abbreviation() {
    let rpc = SigninParams::access("my_ns", "my_db", "account").to_rpc_params();

    assert_eq!(rpc.len(), 3);
    assert_eq!(rpc.get("NS"), Some(&json!("my_ns")));
    assert_eq!(rpc.get("DB"), Some(&json!("my_db")));
    assert_eq!(rpc.get("AC"), Some(&json!("account")));
}

#[test]
fn signin_vars_are_sent_alongside_credentials() {
    let rpc = SigninParams::access("ns", "db", "account")
        .with_var("email", json!("a@example.com"))
        .with_var("pass", json!("pw"))
        .to_rpc_params();

    assert_eq!(rpc.get("email"), Some(&json!("a@example.com")));
    assert_eq!(rpc.get("pass"), Some(&json!("pw")));
    assert_eq!(rpc.get("AC"), Some(&json!("account")));
}

#[test]
fn signin_new_sends_nothing() {
    assert!(SigninParams::new().to_rpc_params().is_empty());
}
