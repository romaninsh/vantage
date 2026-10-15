//! Keying listed rows by id without letting one row replace another.
//!
//! Both drivers hand rows back as an id-keyed map. A row the server sent
//! without an id still needs a key, and a key shared by two rows would keep
//! only one of them.

use indexmap::IndexMap;
use vantage_core::{Result, error};

/// Insert `record` into `rows` under `id`, or under a synthetic key when it
/// has none (`None` or empty).
///
/// The synthetic key is `"~row:<position>"`, where `position` is the row's
/// zero-based index in the server's whole result set (the offset the page
/// was fetched at plus its index on the page), so id-less rows from
/// different pages never share a key. `~` is readable and travels in a URL
/// unencoded; should a real id ever equal a synthetic key, it is caught
/// below like any other duplicate.
///
/// Two rows under the same key are an error naming the id: keeping either
/// would silently drop the other.
pub(crate) fn insert_row<V>(
    rows: &mut IndexMap<String, V>,
    id: Option<String>,
    position: usize,
    record: V,
) -> Result<()> {
    let key = match id {
        Some(id) if !id.is_empty() => id,
        _ => synthetic_key(position),
    };
    if rows.contains_key(&key) {
        return Err(error!("Two rows in the response share an id", id = key));
    }
    rows.insert(key, record);
    Ok(())
}

fn synthetic_key(position: usize) -> String {
    format!("~row:{position}")
}
