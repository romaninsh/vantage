//! Every reader of a capability set sees every `Capability`.
//!
//! Serde and the rhai `capabilities()` map are both generated from
//! `Capability::iter()`: each lists every capability by name, and reads its
//! own bit.

use vantage_vista::{Capability, VistaCapabilities};

/// Every other capability, so a reader that confuses two of them shows.
fn alternating() -> VistaCapabilities {
    Capability::iter().step_by(2).collect()
}

fn expected(caps: &VistaCapabilities) -> Vec<(String, bool)> {
    Capability::iter()
        .map(|c| (c.as_str().to_string(), caps.has(c)))
        .collect()
}

#[test]
fn capability_flag_complete() {
    let caps = alternating();
    let json = serde_json::to_value(caps).unwrap();
    let mut serialized: Vec<(String, bool)> = json
        .as_object()
        .expect("a capability set serializes as a map")
        .iter()
        .map(|(k, v)| (k.clone(), v.as_bool().expect("each value is a bool")))
        .collect();
    let mut want = expected(&caps);
    serialized.sort();
    want.sort();
    assert_eq!(serialized, want, "serde must list every capability");
    let back: VistaCapabilities = serde_json::from_value(json).unwrap();
    assert_eq!(back, caps, "serde must round-trip");

    #[cfg(all(feature = "rhai", feature = "mocks"))]
    rhai_map_lists_every_capability(caps);
}

#[cfg(all(feature = "rhai", feature = "mocks"))]
fn rhai_map_lists_every_capability(caps: VistaCapabilities) {
    use super::rhai_support::{host, mock, reads, run};
    use vantage_rhai::rhai::Map as RhaiMap;

    let host = host(mock().with_capabilities(caps), reads());
    let map = run(&host, r#"table("t").capabilities()"#)
        .unwrap()
        .try_cast::<RhaiMap>()
        .expect("capabilities() returns a map");
    let mut listed: Vec<(String, bool)> = map
        .iter()
        .map(|(k, v)| (k.to_string(), v.as_bool().expect("each value is a bool")))
        .collect();
    // The map reads the Vista's capabilities, where `Delete` implies `DeleteAll`.
    let effective = if caps.has(Capability::Delete) {
        caps.with(Capability::DeleteAll)
    } else {
        caps
    };
    let mut want = expected(&effective);
    listed.sort();
    want.sort();
    assert_eq!(listed, want, "the rhai map must list every capability");
}
