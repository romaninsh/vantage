//! A flash carries the caller's `WriteContext` onto the Dio's write worker.
//!
//! The worker is its own task, so a task-local scope set by the caller ends
//! at the queue. The flash takes the context in scope when it is fired, and
//! the worker runs the write inside it: the master table's hooks, and a Lens
//! writer, see who made the change.

mod support;

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::{Cache, ChangeFlash, Dio, FlashKind, FlashOutcome, Lens};
use vantage_sql::sqlite::SqliteDB;
use vantage_sql::sqlite::vista::SqliteVistaFactory;
use vantage_table::table::{AfterCx, Hook, Table, after_hook};
use vantage_table::write_context::WriteContext;
use vantage_types::EmptyEntity;
use vantage_vista::{FilterOp, Vista};

use support::record;

/// Every context an after-update hook was handed, one entry per write.
type Seen = Arc<Mutex<Vec<Option<WriteContext>>>>;

fn ana() -> WriteContext {
    let principal = CborValue::Map(vec![(
        CborValue::Text("name".into()),
        CborValue::Text("ana".into()),
    )]);
    WriteContext::new()
        .with_principal(principal)
        .with_group("g1")
}

/// A Dio over an in-memory sqlite `products` table (one row, id `1`) whose
/// after-update hook records the context it receives. The connection is
/// returned so it outlives the Dio's writes.
async fn hooked_products() -> Result<(SqliteDB, Dio, Seen)> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .map_err(|e| vantage_core::error!("sqlite connect", detail = e.to_string()))?;
    let db = SqliteDB::new(pool);
    for statement in [
        "CREATE TABLE products (id TEXT PRIMARY KEY, name TEXT)",
        "INSERT INTO products (id, name) VALUES ('1', 'Espresso')",
    ] {
        sqlx::query(statement)
            .execute(db.pool())
            .await
            .map_err(|e| vantage_core::error!("sqlite setup", detail = e.to_string()))?;
    }
    let seen = Seen::default();
    let hook = after_hook({
        let seen = seen.clone();
        move |cx: AfterCx<'_, SqliteDB>| {
            seen.lock().unwrap().push(cx.context.cloned());
            Box::pin(async { Ok(()) })
        }
    });
    let table = Table::<SqliteDB, EmptyEntity>::new("products", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_hook(Hook::AfterUpdate(hook));
    let vista = SqliteVistaFactory::new(db.clone()).from_table::<EmptyEntity>(table)?;
    let dio = support::eager_dio(vista).await;
    Ok((db, dio, seen))
}

#[tokio::test]
async fn write_context_reaches_the_master_table_hook_through_flash() -> Result<()> {
    let (_db, dio, seen) = hooked_products().await?;
    let rename = ChangeFlash::partial("1", record(&[("name", "Latte")]));
    ana().scope(dio.flash(rename)).await?;
    assert_eq!(*seen.lock().unwrap(), vec![Some(ana())]);
    Ok(())
}

#[tokio::test]
async fn flash_without_a_scope_carries_no_context() -> Result<()> {
    let (_db, dio, seen) = hooked_products().await?;
    let rename = ChangeFlash::partial("1", record(&[("name", "Latte")]));
    assert!(rename.context().is_none());
    dio.flash(rename).await?;
    assert_eq!(*seen.lock().unwrap(), vec![None]);
    Ok(())
}

/// What a recording writer saw per flash: its kind and id, the context the
/// flash carried, and the context in scope while the writer ran.
type Entry = (
    FlashKind,
    Option<String>,
    Option<WriteContext>,
    Option<WriteContext>,
);
type Written = Arc<Mutex<Vec<Entry>>>;

/// A Clear on a Dio whose writer route can't confine it runs as one delete
/// per row. The context is attached to the Clear itself and no scope is open,
/// so each delete only has it if the Clear passed it on.
#[tokio::test]
async fn narrowed_clear_carries_context_to_each_row() -> Result<()> {
    let written = Written::default();
    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(support::copy_master())
        .writer({
            let written = written.clone();
            move |_cx, flash| {
                written.lock().unwrap().push((
                    *flash.kind(),
                    flash.id().map(str::to_string),
                    flash.context().cloned(),
                    WriteContext::current(),
                ));
                async { Ok(FlashOutcome::applied()) }
            }
        })
        .build()?;
    let shell = vantage_vista::mocks::MockShell::new()
        .with_metadata(support::meta(&["id", "team"]))
        .with_record("1", record(&[("id", "1"), ("team", "red")]))
        .with_record("2", record(&[("id", "2"), ("team", "red")]))
        .with_record("3", record(&[("id", "3"), ("team", "blue")]));
    let dio = lens.dio(Vista::new("teams", shell)).build().await?;
    let red = dio
        .derive()
        .with_query(&support::cond("team", FilterOp::Eq, "red"))
        .build()
        .await?;

    red.flash(ChangeFlash::clear().with_context(ana())).await?;

    let written = written.lock().unwrap();
    let expected = |id: &str| {
        (
            FlashKind::Delete,
            Some(id.to_string()),
            Some(ana()),
            Some(ana()),
        )
    };
    assert_eq!(*written, vec![expected("1"), expected("2")]);
    Ok(())
}
