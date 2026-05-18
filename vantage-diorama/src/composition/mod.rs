pub mod merge;
pub mod overlay;

use vantage_vista::Vista;

/// Composition primitives that fold two Vistas into one.
///
/// `overlay(a, b)` projects `b`'s records over `a`'s — useful for
/// patching a read-only base with a writable override.
/// `merge(a, b)` concatenates records from both sources — useful for
/// stitching pages from different shards or layering an audit log.
///
/// Stage 1 reserves the namespace. Stage 9 supplies the implementations.
pub struct Diorama;

impl Diorama {
    pub fn overlay(_a: Vista, _b: Vista) -> Vista {
        unimplemented!("Diorama::overlay lands in stage 9");
    }

    pub fn merge(_a: Vista, _b: Vista) -> Vista {
        unimplemented!("Diorama::merge lands in stage 9");
    }
}
