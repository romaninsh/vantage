//! Change subscription through SurrealDB `LIVE SELECT`.

use surreal_client::Action;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Entity;
use vantage_vista::{VistaChange, VistaChangeStream};

use super::{SurrealTableShell, to_cbor_record};
use crate::thing::Thing;
use crate::types::{AnySurrealType, SurrealType};

impl<E> SurrealTableShell<E>
where
    E: Entity<AnySurrealType> + 'static,
{
    /// Watch the wrapped table and stream each change as a [`VistaChange`].
    ///
    /// SurrealDB delivers `CREATE`/`UPDATE`/`DELETE` frames carrying the
    /// affected record's `Thing`. So that a live-updated row has exactly the
    /// shape of a listed one, each `CREATE`/`UPDATE` re-reads the row through
    /// the shell's table rather than reshaping the pushed payload. `DELETE`
    /// needs no read.
    ///
    /// The subscription follows the whole table, which the `watch` contract
    /// allows. The re-read runs through the shell's conditions, so a changed
    /// row outside the set reports `Deleted`.
    ///
    /// A query-sourced vista has no table to subscribe to and doesn't
    /// advertise `Subscribe`, so `Vista::watch` refuses it before this runs.
    pub(super) async fn live_changes(&self) -> Result<VistaChangeStream> {
        let table_name = self.table.table_name().to_string();
        let live = self.table.data_source().live(&table_name).await?;
        let table = self.table.clone();

        let stream = async_stream::try_stream! {
            let mut live = live;
            while let Some(note) = live.recv().await {
                let Some(thing) = Thing::from_cbor(note.record_id.clone()) else {
                    continue;
                };
                let id = thing.to_string();
                match note.action {
                    Action::Delete => yield VistaChange::Deleted { id },
                    Action::Create | Action::Update => {
                        match table.get_value(thing.clone()).await? {
                            Some(record) => {
                                let value = to_cbor_record(record);
                                if note.action == Action::Create {
                                    yield VistaChange::Inserted { id, value };
                                } else {
                                    yield VistaChange::Updated { id, value };
                                }
                            }
                            // Gone between notification and read, or outside
                            // the set: removed from the consumer's view.
                            None => yield VistaChange::Deleted { id },
                        }
                    }
                }
            }
        };
        Ok(Box::pin(stream))
    }
}
