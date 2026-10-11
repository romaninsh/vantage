use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_rhai::rhai::Dynamic;
use vantage_rhai::{Block, Env, Host, Limits};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Column, DataVocab, TableShell, TargetResolver, Terminals, Vista, VistaMetadata,
};

fn rec(a: i64) -> Record<CborValue> {
    [("a".to_string(), CborValue::Integer(a.into()))]
        .into_iter()
        .collect()
}

/// Three rows: r1 {a:1}, r2 {a:2}, r3 {a:1}.
pub fn mock() -> MockShell {
    MockShell::new()
        .with_metadata(
            VistaMetadata::new()
                .with_column(Column::new("id", "String").with_flag("id"))
                .with_column(Column::new("a", "int"))
                .with_id_column("id"),
        )
        .with_record("r1", rec(1))
        .with_record("r2", rec(2))
        .with_record("r3", rec(1))
}

/// Resolves every name to a fresh copy of `shell`, with its own query state.
pub fn resolver(shell: MockShell) -> TargetResolver {
    Arc::new(move |name: &str| Ok(Vista::new(name, shell.clone_shell().unwrap())))
}

pub fn host(shell: MockShell, terminals: Terminals) -> Host {
    Host::builder(Limits::background())
        .vocab(DataVocab {
            resolver: Some(resolver(shell)),
            terminals,
        })
        .build()
}

pub fn run(host: &Host, script: &str) -> Result<Dynamic, String> {
    host.compile_uncached(&Block::from(script))
        .and_then(|s| s.eval(&Env::new()))
        .map_err(|e| e.to_string())
}
