# Transactions

Moving 30 from Alice's account to Bob's takes two patches. Without a transaction, if the second
one fails, Bob has the money and Alice still has it too. A
[`Transaction`](vantage_vista::Transaction) makes the two patches land together or not at all, on
every backend that has transactions, through the same `Vista` API you already write with.

This chapter is the Rust side: the handle, what each backend does with it, what happens across
several databases, and what a driver implements to take part. Rhai's `atomic { }` block is built
on the same handle and gets one short section near the end.

<!-- toc -->

---

## A transfer

Wrap the Vistas you write through, write, commit. The example runs on the in-memory store, where
`account` is narrowed to `balance >= 0`, so a patch that would overdraw fails:

```rust
# use ciborium::Value as CborValue;
# use vantage_core::{Result, error};
# use vantage_memory::{MemoryStore, MemoryVistaFactory};
# use vantage_types::Record;
# use vantage_vista::prelude::*;
# use vantage_vista::VistaFactory;
# const ACCOUNT: &str = r#"
# name: account
# id_column: id
# columns:
#   id: { type: string, flags: [id] }
#   balance: { type: integer }
# "#;
# fn balance(n: i64) -> Record<CborValue> {
#     [("balance".to_string(), CborValue::Integer(n.into()))].into_iter().collect()
# }
# async fn balance_of(accounts: &Vista, id: &str) -> Result<i64> {
#     let row = accounts.get_value(id).await?.ok_or_else(|| error!("no such account"))?;
#     match row.get("balance") {
#         Some(CborValue::Integer(n)) => Ok(i64::try_from(*n).unwrap_or_default()),
#         _ => Err(error!("no balance")),
#     }
# }
use vantage_vista::{Committed, Transaction};

async fn transfer(accounts: &Vista, from: &str, to: &str, amount: i64) -> Result<Committed> {
    let tx = Transaction::new();
    let accounts = tx.wrap(accounts);
    let (a, b) = (balance_of(&accounts, from).await?, balance_of(&accounts, to).await?);
    accounts.patch_value(to, &balance(b + amount)).await?;
    accounts.patch_value(from, &balance(a - amount)).await?; // overdrawn: Err, and `?` drops `tx`
    tx.commit().await
}
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<()> {
# let factory = MemoryVistaFactory::new(MemoryStore::new());
# let accounts = factory
#     .from_yaml(ACCOUNT)?
#     .with_condition("balance", FilterOp::Gte, CborValue::Integer(0.into()))?;
# accounts.insert_value("alice", &balance(100)).await?;
# accounts.insert_value("bob", &balance(100)).await?;

let committed = transfer(&accounts, "alice", "bob", 30).await?;
assert_eq!(committed.written, ["account"]);

assert!(transfer(&accounts, "alice", "bob", 500).await.is_err());
assert_eq!(balance_of(&accounts, "bob").await?, 130); // the +500 never landed
# assert_eq!(balance_of(&accounts, "alice").await?, 70);
# Ok(())
# }
```

The second transfer patches Bob first, then fails on Alice. The early return drops the
transaction and the wrapped Vista, and dropping them rolls back, so Bob's patch is gone too.

## The handle

[`Transaction::new`](vantage_vista::Transaction::new) begins nothing. A transaction is a shared
handle (cheap to clone) that collects the backends its Vistas touch.

- [`tx.wrap(&vista)`](vantage_vista::Transaction::wrap) returns the Vista reading and writing
  through `tx`. Conditions, sort, hooks and references carry over. Every Vista it hands back from
  `get_ref` is wrapped too, so a child row inserted through a relation is in the same
  transaction. Live streams (`Subscribe`, `Cursor`) are switched off on the wrapped Vista: they
  would outlive the transaction.
- [`tx.resolver(base)`](vantage_vista::Transaction::resolver) wraps a whole
  [`VistaResolver`](vantage_vista::VistaResolver): every Vista it resolves or adopts comes back
  wrapped. A host hands it to code that looks tables up by name.
- [`tx.commit()`](vantage_vista::Transaction::commit) commits every backend that joined and
  returns [`Committed`](vantage_vista::Committed): `written` lists the tables written through the
  transaction, in first-written order, and `unprotected` the tables written directly by datasources
  without transactions (see [below](#datasources-without-transactions)).
- [`tx.rollback()`](vantage_vista::Transaction::rollback) rolls them back. It is always `Ok`, and
  calling it on a finished transaction does nothing, so cleanup paths can call it freely.
- Dropping the transaction and every Vista wrapped in it rolls back. Each backend's open
  transaction rolls back when its handle drops (sqlx queues a `ROLLBACK`, MongoDB aborts the
  session, SurrealDB closes the connection).

A backend's own transaction starts at the first statement, not at `wrap`: wrapping a Vista you
end up not using costs nothing. After commit or rollback, a wrapped Vista answers every call with
`transaction finished`.

```rust
# use std::sync::Arc;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::{Transaction, Vista, VistaResolver};
# fn main() -> vantage_core::Result<()> {
let base: Arc<dyn VistaResolver> =
    Arc::new(|name: &str| Ok(Vista::new(name, MockShell::new())));
let tx = Transaction::new();
let tables = tx.resolver(base);
let orders = tables.vista("order")?; // wrapped in `tx`
# let _ = orders;
# Ok(())
# }
```

A Vista whose shell can't take part still wraps without an error. The refusal comes at its first
use: `"<table>" can't take part in a transaction`. `wrap` doesn't return a `Result` because a host
wraps whole resolvers up front, and most of the tables it wraps are never touched.

### A failed statement fails the commit

When any statement inside the transaction fails, `commit` rolls back and returns
`a statement failed inside the transaction; rolled back`. That includes an error you caught and
moved past, and a statement dropped mid-flight by a timeout or a `select!`, which the server may
still have run.

#### Why not commit what did succeed?

Postgres turns a `COMMIT` after an error into a rollback. MySQL, SQLite and SurrealDB don't: the
failed statement is undone on its own, and the rest commit. Code that caught the error might then commit half of
what it meant to write, and the result would depend on the database. Vantage keeps one rule for all
of them. If you want to try a write that may fail and carry on regardless, do it outside the
transaction.

The in-memory store goes further: any failed operation fails its commit, including a refusal from
the Vista layer (a write outside the set, say) that never reached the store.

## After-hooks wait for the commit

A table's after-hooks (`AfterInsert`, `AfterUpdate`, `AfterSave`, `AfterDelete`,
`AfterDeleteAll`; see [Records](./record-lifecycle.md#lifecycle-hooks)) don't run at the write
inside a transaction. They run once the commit succeeds, in order, outside the transaction, under
the [`WriteContext`](vantage_table::write_context::WriteContext) the write had. A rollback drops
them. An after-hook that fails at that point is logged with `tracing::warn!`: the rows are already
committed, so `commit` returns `Ok`.

Before-hooks run at the write, inside the transaction, as they always do. A before-hook that
returns `Err` fails the write. If you let the error leave the transaction, it rolls back. If you
catch it and commit, the commit goes ahead on SQL, MongoDB and SurrealDB, because only a failed
database statement fails the commit there. On memory, any failed operation fails it.

## Locking reads

At Postgres's default isolation (read committed), a read-then-write inside a transaction can still
race another writer: both read 10, both write 9. [`Vista::lock`](vantage_vista::Vista::lock) makes
the wrapped Vista's reads lock the rows they return until the transaction ends:

```rust,no_run
# use vantage_vista::prelude::*;
# use vantage_vista::Transaction;
# async fn reserve(stock: &Vista) -> vantage_core::Result<()> {
let tx = Transaction::new();
let mut stock = tx.wrap(stock);
stock.lock()?;
let row = stock.get_value("sku-1").await?; // SELECT … FOR UPDATE on Postgres and MySQL
// … patch the row from what was read
# let _ = row;
tx.commit().await?;
# Ok(())
# }
```

- Postgres and MySQL read `FOR UPDATE`. A second locked reader of the row waits until the first
  transaction ends, then sees its write.
- SQLite does nothing more: its transactions begin with `BEGIN IMMEDIATE`, which already holds
  the database's write lock.
- A SQL Vista that isn't wrapped refuses with `lock() needs a transaction`.
- Other backends refuse with `this shell can't lock rows`.

There is no isolation setting. Each database runs at its own default: read committed on Postgres,
repeatable read on MySQL, MongoDB's default read concern, serializable on SQLite. `lock()` is the tool for
the read-then-write case.

## What each backend does

| backend | inside a transaction | notes |
| --- | --- | --- |
| Postgres, MySQL | one connection from the pool, `BEGIN` | deadlocks are reported by the server and fail the write |
| SQLite | one connection, `BEGIN IMMEDIATE` | writers queue on the file lock instead of deadlocking |
| MongoDB | a session's transaction | replica sets and sharded clusters only |
| SurrealDB | an interactive transaction on its own connection | SurrealDB 3 and later |
| memory | a claim on the store and copies of the touched tables | see below |
| SpacetimeDB, log-writer | write directly | listed in `unprotected` |
| CSV, REST, GraphQL, Kubernetes, AWS, cmd | direct, read-only | a transaction only reads them |

**MongoDB** needs a replica set (a single-node one is fine) for multi-document transactions.
[`MongoDB::connect`](vantage_mongodb::MongoDB::connect) asks the server with `hello`; a standalone
server then writes directly, as a datasource without transactions. A handle built with
`MongoDB::new` doesn't ask, so its Vistas join, and on a standalone server the first operation in
the transaction fails. A write to a document another transaction holds fails at once with a
`WriteConflict`, which surfaces as an error and rolls the transaction back. Nothing retries it.

**SurrealDB** runs the transaction with the `begin`, `commit` and `cancel` RPCs on a connection of
its own: every clone of a `SurrealDB` shares one socket, and a transaction kept there would hold it
for the whole block. That needs a client made by `SurrealConnection::connect`, which keeps the
connection settings to open a second socket, and a SurrealDB 3+ server. Any other client writes
directly. SurrealDB is optimistic: two transactions writing the same record both go ahead, and
the second to commit fails with a conflict.

**Memory** has no transactions of its own, so the driver builds one per store. On first use, read
or write, the transaction claims the store. Another transaction on the same store waits at its own
first use, for up to 2 seconds (`MemoryStore::with_transaction_wait`), then fails with
`the memory store is busy with another transaction`. Each touched table is copied, and the
transaction reads and writes the copy. Commit replays the transaction's row changes onto the
store's tables as they are by then, so a write made outside meanwhile stays. An insert whose id was
taken outside meanwhile fails the commit, with nothing applied. Callers outside a transaction never
wait on the claim. Two transactions on one store run from the same task do: the second waits for a
claim the first holds until it finishes, and gets the busy error.

**DynamoDB and redb** aren't in the table. Neither has a Vista shell (DynamoDB has a
`TableSource`, redb is a Dio cache), so no Vista over them can be wrapped.

## Several databases

One transaction can span several datasources: wrap Vistas from each. Each datasource joins when
its first Vista is wrapped, and at commit they commit one after another in that order.

- If one fails before any has committed, all of them roll back.
- If one fails after others committed, the rest roll back and the earlier ones stay committed. The
  error names their tables, and
  [`Transaction::committed_before_failure`](vantage_vista::Transaction::committed_before_failure)
  reads them back out:

```rust,no_run
# use vantage_types::Record;
# use vantage_vista::prelude::*;
# use vantage_vista::Transaction;
# async fn ship(
#     orders: &Vista,
#     stock: &Vista,
#     order: &Record<CborValue>,
#     left: &Record<CborValue>,
# ) -> vantage_core::Result<()> {
let tx = Transaction::new();
let (orders, stock) = (tx.wrap(orders), tx.wrap(stock)); // e.g. Postgres, then MongoDB
orders.insert_value("o-1", order).await?;
stock.patch_value("sku-1", left).await?;
if let Err(e) = tx.commit().await {
    let landed = Transaction::committed_before_failure(&e); // ["order"] if only Postgres committed
    // undo or report what landed
    # let _ = landed;
    return Err(e);
}
# Ok(())
# }
```

Since order matters, wrap the datasource whose commit is likelier to fail first. If it fails,
nothing has landed yet.

### Why not two-phase commit?

Two-phase commit closes that window, but it needs a decision log: a durable record of "prepared,
about to commit" that a recovering process reads to finish or undo the second phase. That log has
to live in a database, and Vantage creates no tables in a client's databases. So a cross-database
commit is atomic per database, and the failure window is reported instead of hidden.

## Datasources without transactions

A shell over a datasource without transactions returns [`direct`](vantage_vista::direct) from its
`rebind`. Its wrapped Vista writes as it always does: each write lands when made, and no rollback
undoes it. The transaction lists those tables in `Committed::unprotected`, and
[`tx.unprotected()`](vantage_vista::Transaction::unprotected) has them after a rollback or a failed
commit too. Rhai hosts log each one as
`atomic: "<table>" has no transactions; its writes landed directly`.

That is why idempotence still matters. A direct write that landed before a rollback is still there
on the retry, so the retry has to be safe to run again: an insert with a client-made id lands on
the same row, a patch to a fixed value is the same patch twice. The same goes for a retry after a
commit that failed part-way across databases.

## What doesn't join

- **Dio.** A Dio's writes run on its worker task and go through its cache. A Dio shell wraps, and
  is refused at its first use (`"<table>" can't take part in a transaction`). Wrap the master Vista the Dio is built over,
  and refresh the Dio after the commit from `Committed::written` and `unprotected`. The Rhai hosts
  in vantage-server and vantage-ui do that for you, also for the tables that landed before a
  part-way failure.
- **Contained relations.** A Vista over records embedded in a parent row refuses (its shell has
  no `rebind`). Write the parent row through a wrapped Vista instead.
- **A forwarding shell that doesn't override `rebind`.** Refused, because only the wrapper knows how
  to rebuild itself over a rebound inner shell (see [below](#rebind)).
- **DynamoDB and redb**, as above.

## In Rhai: `atomic { }`

Scripts get the same handle as a block:

```rhai
atomic {
    let accounts = table("account");
    let alice = accounts.get("alice");
    accounts.patch("bob", #{ balance: accounts.get("bob").balance + 30 });
    if alice.balance < 30 { throw "not enough money"; }
    accounts.patch("alice", #{ balance: alice.balance - 30 });
} rollback (e, committed) {
    log(`transfer failed: ${e}; already committed: ${committed}`);
}
```

Every table handle inside the block goes through one transaction per database, including handles
made before the block. The block commits when it ends, or on `return` or `exit`. A `throw`, a
`break` or `continue` out of it, a cancel, or its 30-second limit rolls it back. The optional
`rollback (e)` or `rollback (e, committed)` arm then runs with a 10-second budget, and a cancel
waits until it ends. `e` is what was thrown, or the error's text. `committed` is the array of
tables that landed before a later database failed, `[]` otherwise. The arm may call actions; if it
throws, the block's original error is the one re-thrown. An action marked
`atomic: true` (`kind: script` only) runs its whole body as one block.

Inside a block, `actions.x()`, a nested `atomic`, contained relations and Dio-only tables are
refused. SQL lock waits and `secrets.get()` consent prompts aren't cut off by the 30-second limit, so
keep blocks short. Nothing retries a failed block.

When to use a block is a rule for script authors, in the **vantage-rhai** skill; the runtime
doesn't check it. A Rust host wires the block with
[`ResolverCell`](vantage_vista::rhai::ResolverCell),
[`AtomicScope`](vantage_vista::rhai::AtomicScope) and
[`AtomicVocab`](vantage_vista::rhai::AtomicVocab), and ends a block past its limit from
`on_progress` through [`AtomicClock::should_stop`](vantage_vista::rhai::AtomicClock::should_stop).

## For driver authors

### `rebind`

[`TableShell::rebind`](vantage_vista::TableShell::rebind) is the whole hook. `wrap` calls it, and
the shell returns itself rebuilt to run on the transaction. There are three shapes.

A datasource with transactions joins its participant there, keyed by a token every clone of the
datasource shares, and returns a shell over the joined datasource. SQL's is two lines, the table
keeping its hooks, conditions and references:

```rust,ignore
fn rebind(&self, tx: &Transaction) -> Result<Box<dyn TableShell>> {
    let data_source = self.table.data_source().in_transaction(tx)?; // tx.participant(key, …)
    Ok(Box::new(Self { table: self.table.with_data_source(data_source), ..self.clone() }))
}
```

Join in `rebind`, not at the first statement. The transaction notes which participants each
rebind joined, and that is how it knows which tables landed when a later database fails. A
participant joined later never shows up in `committed`.

A datasource without transactions returns `Ok(direct(Box::new(self.clone())))`. A driver whose
datasource has transactions only sometimes (MongoDB standalone, an old SurrealDB) decides per call.

A forwarding shell rebinds its inner shell and wraps the result in a copy of itself. The default
`rebind` refuses, and `forward_table_shell!` doesn't forward it, so a wrapper has to write it:

```rust
use vantage_core::Result;
use vantage_vista::{TableShell, Transaction};

#[derive(Clone)]
struct Audited {
    inner: Box<dyn TableShell>,
}

vantage_vista::forward_table_shell!(Audited, inner, {
    fn rebind(&self, tx: &Transaction) -> Result<Box<dyn TableShell>> {
        Ok(Box::new(Self { inner: self.inner.rebind(tx)? }))
    }
});
```

A shell that can lock rows also overrides `set_lock` (the default refuses). A
[`TableSource`](vantage_table::traits::table_source::TableSource) in transaction mode answers
`transaction()` with its transaction and `without_transaction()` with itself back on its pool:
that is what holds after-hooks until the commit.

### `TxSlot` and `TxBackend`

Most of a participant is the same whatever the backend: begin on first use, run one operation at a
time, remember a failed one, refuse use after commit, roll back instead of committing after a
failure. [`TxSlot`](vantage_vista::TxSlot) does all of that. A driver implements
[`TxBackend`](vantage_vista::TxBackend): how to begin, commit and roll back. SQL, MongoDB and
SurrealDB are each one `TxBackend` of about 40 lines.

```rust
# use std::sync::{Arc, Mutex};
# use async_trait::async_trait;
# use vantage_core::Result;
use vantage_vista::{Transaction, TxBackend, TxKey, TxSlot};

/// Stands in for the driver's connection; it records what it ran.
struct Conn(Arc<Mutex<Vec<String>>>);

impl Conn {
    async fn run(&mut self, sql: &str) -> Result<()> {
        self.0.lock().unwrap().push(sql.to_string());
        Ok(())
    }
}

struct Begin(Arc<Mutex<Vec<String>>>);

#[async_trait]
impl TxBackend for Begin {
    type Open = Conn;

    async fn begin(&self) -> Result<Conn> {
        let mut conn = Conn(self.0.clone());
        conn.run("BEGIN").await?;
        Ok(conn)
    }
    async fn commit(&self, mut conn: Conn) -> Result<()> {
        conn.run("COMMIT").await
    }
    async fn rollback(&self, mut conn: Conn) -> Result<()> {
        conn.run("ROLLBACK").await
    }
}

#[derive(Clone)]
struct Db {
    log: Arc<Mutex<Vec<String>>>,
    key: Arc<()>,
    tx: Option<Arc<TxSlot<Begin>>>,
}

impl Db {
    /// What the shell's `rebind` calls.
    fn in_transaction(&self, tx: &Transaction) -> Result<Self> {
        let slot = tx.participant(TxKey::of(&self.key), || TxSlot::new(Begin(self.log.clone()), tx))?;
        Ok(Self { tx: Some(slot), ..self.clone() })
    }

    async fn execute(&self, sql: &str) -> Result<()> {
        match &self.tx {
            None => Conn(self.log.clone()).run(sql).await,
            Some(slot) => {
                let mut open = slot.open().await?; // begins on first use
                slot.watch(open.run(sql)).await // an Err, or a drop mid-flight, fails the commit
            }
        }
    }
}
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<()> {
# let db = Db { log: Arc::default(), key: Arc::new(()), tx: None };

let tx = Transaction::new();
let inside = db.in_transaction(&tx)?;
inside.execute("INSERT …").await?;
tx.commit().await?;
assert_eq!(*db.log.lock().unwrap(), ["BEGIN", "INSERT …", "COMMIT"]);
# Ok(())
# }
```

The slot holds a weak handle to its transaction, so dropping the transaction drops the backend's
open handle too. A backend with no transactions to begin, like memory, implements
[`TxParticipant`](vantage_vista::TxParticipant) itself instead.

### The contract

[`contract::check_transactions`](vantage_vista::contract::check_transactions) runs the
transaction probes against a [`TxFixture`](vantage_vista::contract::TxFixture): commit, rollback,
drop, isolation from outside readers, lazy begin, `get_ref` targets, concurrent calls, a failed
statement, use after finish, two crossed writers, `lock()`, and two datasources at once.
[`contract::check_atomic_rhai`](vantage_vista::contract::check_atomic_rhai) runs the same fixture
through Rhai `atomic` blocks. Each check panics once, listing every probe that broke, and finishes
inside 4.5 seconds.

The fixture hands out two empty tables, `a(id, n)` and `b(id, a_id, n)`, with `n >= 0` enforced
and a has-many relation `b` from `a`. The memory driver's, on a store per call:

```rust,no_run
# use async_trait::async_trait;
# use vantage_memory::{MemoryStore, MemoryVistaFactory};
# use vantage_vista::{FilterOp, VistaFactory};
use vantage_vista::contract::{self, TxFixture, TxTables};
# const TX_A: &str = "name: a\nid_column: id\ncolumns:\n  id: { type: string, flags: [id] }\n  n: { type: integer }\nreferences:\n  b: { table: b, kind: has_many, foreign_key: a_id }\n";
# const TX_B: &str = "name: b\nid_column: id\ncolumns:\n  id: { type: string, flags: [id] }\n  a_id: { type: string }\n  n: { type: integer }\n";

struct StoreTx;

#[async_trait]
impl TxFixture for StoreTx {
    async fn fresh(&self) -> TxTables {
        let f = MemoryVistaFactory::new(MemoryStore::new());
        let a = f.from_yaml(TX_A).unwrap();
        // The store checks no constraints; the set does it instead.
        let a = a.with_condition("n", FilterOp::Gte, ciborium::Value::Integer(0.into())).unwrap();
        TxTables::new(a, f.from_yaml(TX_B).unwrap())
    }
    async fn fresh_pool_of_one(&self) -> TxTables {
        self.fresh().await
    }
    fn detects_deadlocks(&self) -> bool {
        false
    }
    async fn fresh_other(&self) -> Option<TxTables> {
        Some(self.fresh().await) // another store: a second participant
    }
}

#[tokio::test]
async fn memory_transactions_hold_the_contract() {
    contract::check_transactions(&StoreTx).await
}

#[tokio::test]
async fn memory_atomic_holds_the_contract() {
    contract::check_atomic_rhai(&StoreTx).await
}
# fn main() {}
```

The other switches: `supports_lock()` for a backend whose `lock()` locks rows,
`conflicts_fail_fast()` for one whose crossed writers both fail on a conflict instead of one
waiting (MongoDB), and `supports_transactions() == false` for a direct datasource, which runs
only the direct-write probe (or the direct-read one when its tables take no inserts).

## Checklist

- Wrap every Vista you write through with `tx.wrap`, or the resolver with `tx.resolver`; an
  unwrapped Vista writes outside the transaction.
- Commit explicitly. Every other way out (an error, `?`, a panic, a drop) rolls back.
- A caught error inside the transaction still fails the commit.
- After-hooks run after the commit; their failures are logged, not returned.
- Use `lock()` for read-then-write on Postgres and MySQL.
- Across databases, the commit is per database: read `committed_before_failure` and wrap the
  likelier-to-fail database first.
- Check `Committed::unprotected`: those writes landed directly, so they have to be safe to retry.
- Dio, contained relations, DynamoDB and redb don't take part.
- A driver: join in `rebind`, build on `TxSlot`, return `direct` when there are no transactions,
  and pass `check_transactions` and `check_atomic_rhai`.
