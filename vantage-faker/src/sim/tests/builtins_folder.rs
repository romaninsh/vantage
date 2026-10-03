//! `builtin:folder_tree`: a flat `nodes` table of log folders and files
//! whose folder sizes follow their children.

use super::*;

fn tree() -> (SimEngine, MemoryTableHandle) {
    let store = MemoryStore::new();
    store.define(
        "nodes",
        vantage_memory::TableDef {
            id_column: "path".into(),
            indexed: vec!["parent".into()],
            id_prefix: None,
        },
    );
    let def = SimDef::new(
        "logs",
        "nodes",
        crate::sim::builtin::builtin("folder_tree").unwrap(),
    )
    .with_args(serde_json::json!({ "chunk_threshold": 200000 }));
    let engine = SimEngine::builder()
        .store(&store)
        .sim(def)
        .manual_clock(start())
        .seed(2)
        .start()
        .unwrap();
    (engine, store.table("nodes"))
}

#[test]
fn chunks_roll_and_parents_follow_children() {
    let (engine, nodes) = tree();
    run_for(&engine, 600, 10);
    let chunks = nodes
        .ids()
        .into_iter()
        .filter(|p| p.contains("chunk_"))
        .count();
    assert!(chunks >= 2, "a chunk rolled over: {chunks}");
    let root = nodes
        .ids()
        .into_iter()
        .find(|p| {
            nodes
                .get(p)
                .map(|r| text(&r, "parent").is_empty())
                .unwrap_or(false)
        })
        .unwrap();
    let children_size: f64 = nodes
        .ids()
        .into_iter()
        .filter_map(|p| nodes.get(&p))
        .filter(|r| text(r, "parent") == root)
        .map(|r| num(&r, "size"))
        .sum();
    assert_eq!(num(&nodes.get(&root).unwrap(), "size"), children_size);
}

#[test]
fn a_parent_filter_lists_one_folder() {
    let (engine, nodes) = tree();
    run_for(&engine, 60, 10);
    let q = vantage_memory::Query::new().filter(vantage_memory::MemoryCondition::cmp(
        "parent",
        vantage_vista::FilterOp::Eq,
        ciborium::Value::Text(String::new()),
    ));
    let roots = nodes.query(&q).unwrap();
    assert_eq!(roots.len(), 1);
}
