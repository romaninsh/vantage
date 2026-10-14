use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_rhai::rhai::Dynamic;
use vantage_rhai::{Block, Env, Host, Limits};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::rhai::{DataVocab, Terminals, Writes};
use vantage_vista::{Column, ColumnFlag, TableShell, Vista, VistaMetadata, VistaResolver};

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
                .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
                .with_column(Column::new("a", "int"))
                .with_id_column("id"),
        )
        .with_record("r1", rec(1))
        .with_record("r2", rec(2))
        .with_record("r3", rec(1))
}

/// Resolves every name to a fresh copy of `shell`, with its own query state.
pub fn resolver(shell: impl TableShell + Clone + 'static) -> Arc<dyn VistaResolver> {
    Arc::new(move |name: &str| Ok(Vista::new(name, shell.clone())))
}

/// Read terminals with `list()` uncapped; every write throws.
pub fn reads() -> Terminals {
    Terminals::ReadWrite {
        limit: None,
        writes: Writes::Denied("this host only reads".into()),
    }
}

pub fn host(shell: impl TableShell + Clone + 'static, terminals: Terminals) -> Host {
    Host::builder(Limits::background())
        .vocab(DataVocab::new(terminals).with_resolver(resolver(shell)))
        .build()
}

pub fn run(host: &Host, script: &str) -> Result<Dynamic, String> {
    host.compile_uncached(&Block::from(script))
        .and_then(|s| s.eval(&Env::new()))
        .map_err(|e| e.to_string())
}
