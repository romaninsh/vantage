//! [`Page::from_listing`]: a window cut from a shell's full listing, for a
//! shell that can't push the window to its backend.

use ciborium::Value as CborValue;
use vantage_types::Record;

use super::Page;

impl Page {
    /// Rows `offset..offset + limit` of `rows`, the shell's full listing
    /// (an `IndexMap` or a `Vec` of `(id, record)`) in the set's order, with
    /// the listing's length as the total.
    ///
    /// Any window is cut. `Vista` refuses an `offset` above 0 before the
    /// shell is called unless the shell advertises
    /// [`Window`](crate::Capability::Window), so advertising it stays the
    /// shell's own choice.
    pub fn from_listing<I>(rows: I, offset: usize, limit: usize) -> Page
    where
        I: IntoIterator<Item = (String, Record<CborValue>)>,
        I::IntoIter: ExactSizeIterator,
    {
        let rows = rows.into_iter();
        let total = rows.len() as i64;
        Page::new(rows.skip(offset).take(limit).collect()).with_total(total)
    }
}
