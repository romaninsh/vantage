//! The capability set a SpacetimeDB relation advertises.

use vantage_vista::{Capability, VistaCapabilities};

use crate::schema::TableKind;

/// The honest capability set for a SpacetimeDB relation.
///
/// Everything left out is left out because the server genuinely can't do it,
/// and this driver doesn't emulate:
///
/// - `Order`: the dialect has no `ORDER BY`.
/// - `Search`: no `LIKE`, so no substring match to push down.
/// - `Window`, `Cursor`: `LIMIT` exists but `OFFSET` doesn't, so there is no way
///   to skip to a window, and no key order to seek through.
/// - `Traverse`: a SpacetimeDB module declares no foreign keys (the ABI has
///   primary keys, unique constraints and indexes, nothing that points at
///   another table), so a relation carries no references.
/// - `ConfineWrites`: inserts aren't conformed to the conditions, and an update
///   can move a row out of the set; see the shell's `writes` module.
/// - `Import`, `NativeConditions`: no atomic batch, no expression vocabulary.
///
/// `FilterOps` is advertised because `WHERE` takes the six comparison
/// operators; `InSet`, `NotInSet` and `Like` are refused one by one.
///
/// Subscriptions need no setup on this backend (no trigger to install, no
/// extension to enable), so `Subscribe` is advertised without an opt-in.
///
/// The writes key off **database ownership**, not merely holding a token:
/// SpacetimeDB refuses SQL DML from any caller who isn't the owner. A view has
/// nothing to write to, and a keyless table has no `WHERE` that names a row.
pub(super) fn capabilities_for(
    kind: TableKind,
    caller_is_owner: bool,
    keyed: bool,
) -> VistaCapabilities {
    let read = [Capability::FilterOps, Capability::Subscribe];
    let writes = [Capability::Insert, Capability::Update, Capability::Delete];
    let writable = caller_is_owner && keyed && matches!(kind, TableKind::Table);
    read.into_iter()
        .chain(writes.into_iter().filter(|_| writable))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_writes(caps: VistaCapabilities) -> bool {
        caps.has(Capability::Insert) || caps.has(Capability::Update) || caps.has(Capability::Delete)
    }

    #[test]
    fn views_are_never_writable_even_for_the_owner() {
        let caps = capabilities_for(TableKind::View, true, true);
        assert!(!has_writes(caps));
        assert!(
            caps.has(Capability::Subscribe),
            "views are still subscribable"
        );
    }

    #[test]
    fn tables_are_writable_only_for_the_owner_and_only_when_keyed() {
        // Not "has a token": SpacetimeDB refuses SQL DML from any caller who is
        // not the owner, so a token alone must not turn these on.
        let owned = capabilities_for(TableKind::Table, true, true);
        assert!(owned.has(Capability::Insert) && owned.has(Capability::Update));
        assert!(owned.has(Capability::Delete));
        assert!(!has_writes(capabilities_for(TableKind::Table, false, true)));
        assert!(!has_writes(capabilities_for(TableKind::Table, true, false)));
    }

    /// Pinned, so nobody "helpfully" emulates one later: the capability and
    /// the behaviour have to move together.
    #[test]
    fn the_capabilities_this_backend_lacks_stay_off() {
        let caps = capabilities_for(TableKind::Table, true, true);
        for missing in [
            Capability::Order,
            Capability::Search,
            Capability::Window,
            Capability::Cursor,
            Capability::Traverse,
            Capability::ConfineWrites,
            Capability::Import,
            Capability::NativeConditions,
        ] {
            assert!(!caps.has(missing), "{missing} must stay off");
        }
        assert!(caps.has(Capability::FilterOps));
    }
}
