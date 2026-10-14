//! The read as it stands, rendered without sending it.

use vantage_vista::Preview;

use super::MongoTableShell;
use crate::mongodb::impls::joins::{join_alias_of, table_joins};

impl MongoTableShell {
    /// Collection, filter, projection, sort and limit, spelled the way
    /// `MongoSelect::preview` spells them: a `find`, or an `aggregate` over a
    /// pipeline view or joined columns. Filters are summarised by count.
    pub(super) fn render_preview(&self) -> Preview {
        let source = self.table.data_source();
        let mut select = self.table.select();
        select.collection = Some(
            source
                .read_collection_name(self.table.table_name())
                .to_string(),
        );
        // Both factory entry points resolve the joins before building the
        // vista, so a broken chain shouldn't get this far. If one does, the
        // preview says the joins are missing rather than rendering without them.
        let note = match table_joins(&self.table) {
            Ok(joins) => {
                select.joins = joins;
                None
            }
            Err(e) => Some(format!("joins could not be resolved: {e}")),
        };
        for field in &mut select.fields {
            if let Some(alias) = join_alias_of(&self.table, field) {
                *field = alias;
            }
        }
        let preview = Preview::new().with_query(select.preview());
        match note {
            Some(note) => preview.with_note(note),
            None => preview,
        }
    }
}
