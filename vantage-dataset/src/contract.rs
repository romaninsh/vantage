//! Contract checks for writable value sets (feature `contract`).
//!
//! Every writable backend runs these against its own storage, so the write
//! contract on [`WritableValueSet`] is stated once and held everywhere. Each
//! check panics with a message naming the broken rule.
//!
//! Before each `check_*` on a [`Fixture`], seed through `all`:
//! `in1` = `{name: "a", parent: "p1"}` (inside `set`) and
//! `out1` = `{name: "b", parent: "p2"}` (outside). `set` is the same storage
//! narrowed by `parent = "p1"`.
//! Before [`check_operator_condition`], seed `cheap` = `{price: 5}` and
//! `dear` = `{price: 20}`; `set` is narrowed by `price > 10`.

use vantage_core::{Result, VantageError};
use vantage_types::{InvariantValue, Record};

use crate::traits::{ReadableValueSet, WritableValueSet};

pub struct Fixture<'a, S: ReadableValueSet + WritableValueSet> {
    pub all: &'a S,
    pub set: &'a S,
    pub id: fn(&str) -> S::Id,
    pub text: fn(&str) -> S::Value,
    /// `false` for an eventually-consistent backend that can't see an id held
    /// outside the set before it writes: the outside insert/replace then only
    /// has to leave that row untouched.
    pub detects_outside: bool,
}

pub struct OpFixture<'a, S: ReadableValueSet + WritableValueSet> {
    pub all: &'a S,
    pub set: &'a S,
    pub id: fn(&str) -> S::Id,
    pub text: fn(&str) -> S::Value,
    pub int: fn(i64) -> S::Value,
}

fn rec<V>(pairs: Vec<(&str, V)>) -> Record<V> {
    let mut r = Record::new();
    for (k, v) in pairs {
        r.insert(k.to_string(), v);
    }
    r
}

fn has<V: InvariantValue>(r: &Record<V>, col: &str, expected: &V) -> bool {
    r.get(col).is_some_and(|v| v.value_eq(expected))
}

/// The error of a write that must fail; panics with `rule` if it succeeded.
/// Stands in for `expect_err`, which would need `Debug` on every backend's
/// value type.
fn fails<T>(result: Result<T>, rule: &str) -> VantageError {
    match result {
        Ok(_) => panic!("{rule}"),
        Err(e) => e,
    }
}

async fn row<S>(s: &S, id: S::Id) -> Option<Record<S::Value>>
where
    S: ReadableValueSet + Sync,
{
    s.get_value(id).await.expect("get_value must not fail")
}

/// For sets without narrowing (e.g. `ImTable`): idempotent delete, insert
/// no-op on an existing id, replace creates, patch of a missing row is NotFound.
pub async fn check_unnarrowed<S>(all: &S, id: fn(&str) -> S::Id, text: fn(&str) -> S::Value)
where
    S: ReadableValueSet + WritableValueSet + Sync,
    S::Value: InvariantValue,
{
    all.delete(id("ghost"))
        .await
        .expect("delete of a missing row must succeed");
    all.insert_value(id("k1"), &rec(vec![("name", text("a"))]))
        .await
        .expect("insert");
    let again = all
        .insert_value(id("k1"), &rec(vec![("name", text("b"))]))
        .await
        .expect("insert of an existing id must succeed");
    assert!(
        has(&again, "name", &text("a")),
        "insert of an existing id must return the stored row"
    );
    assert!(
        has(&row(all, id("k1")).await.unwrap(), "name", &text("a")),
        "insert must not overwrite"
    );
    all.replace_value(id("k2"), &rec(vec![("name", text("c"))]))
        .await
        .expect("replace creates");
    assert!(
        row(all, id("k2")).await.is_some(),
        "replace of a missing row must create it"
    );
    let e = fails(
        all.patch_value(id("ghost"), &rec(vec![("name", text("x"))]))
            .await,
        "patch of a missing row must fail",
    );
    assert!(
        e.is_not_found(),
        "patch of a missing row must be NotFound: {e}"
    );
    all.delete(id("k1")).await.expect("delete");
    all.delete(id("k1"))
        .await
        .expect("second delete must succeed");
}

pub async fn check_delete<S>(f: &Fixture<'_, S>)
where
    S: ReadableValueSet + WritableValueSet + Sync,
    S::Value: InvariantValue,
{
    f.set
        .delete((f.id)("out1"))
        .await
        .expect("delete of a row outside the set must report success");
    assert!(
        row(f.all, (f.id)("out1")).await.is_some(),
        "delete must not touch a row outside the set"
    );
    f.set
        .delete((f.id)("ghost"))
        .await
        .expect("delete of a missing row must succeed");
    f.set
        .delete((f.id)("in1"))
        .await
        .expect("delete of a row in the set must succeed");
    assert!(
        row(f.all, (f.id)("in1")).await.is_none(),
        "the row in the set must be gone"
    );
    f.set
        .delete((f.id)("in1"))
        .await
        .expect("a retried delete must succeed");
}

pub async fn check_insert<S>(f: &Fixture<'_, S>)
where
    S: ReadableValueSet + WritableValueSet + Sync,
    S::Value: InvariantValue,
{
    let t = f.text;
    let stored = f
        .set
        .insert_value((f.id)("in1"), &rec(vec![("name", t("changed"))]))
        .await
        .expect("insert of an id already in the set must succeed");
    assert!(
        has(&stored, "name", &t("a")),
        "insert of an existing id must return the stored row"
    );
    assert!(
        has(&row(f.all, (f.id)("in1")).await.unwrap(), "name", &t("a")),
        "insert must not overwrite"
    );

    let outside = f
        .set
        .insert_value((f.id)("out1"), &rec(vec![("name", t("x"))]))
        .await;
    if f.detects_outside {
        let e = fails(outside, "insert of an id held outside the set must fail");
        assert!(
            e.is_conflict(),
            "an id held outside the set is a Conflict: {e}"
        );
    }
    let out = row(f.all, (f.id)("out1")).await.expect("out1 still there");
    assert!(
        has(&out, "name", &t("b")) && has(&out, "parent", &t("p2")),
        "an insert must never overwrite a row outside the set"
    );

    f.set
        .insert_value((f.id)("new1"), &rec(vec![("name", t("n"))]))
        .await
        .expect("insert of a new id must succeed");
    assert!(
        has(
            &row(f.all, (f.id)("new1")).await.expect("new1 stored"),
            "parent",
            &t("p1")
        ),
        "an insert into the set must fill the set's equality condition"
    );
    f.set
        .insert_value((f.id)("new1"), &rec(vec![("name", t("n"))]))
        .await
        .expect("a retried insert with the same id must succeed");

    let e = fails(
        f.set
            .insert_value(
                (f.id)("new2"),
                &rec(vec![("name", t("n")), ("parent", t("p2"))]),
            )
            .await,
        "an insert whose payload names another set must fail",
    );
    assert!(e.is_conflict(), "{e}");
    assert!(
        row(f.all, (f.id)("new2")).await.is_none(),
        "a rejected insert must store nothing"
    );
}

pub async fn check_patch<S>(f: &Fixture<'_, S>)
where
    S: ReadableValueSet + WritableValueSet + Sync,
    S::Value: InvariantValue,
{
    let t = f.text;
    let patched = f
        .set
        .patch_value((f.id)("in1"), &rec(vec![("name", t("p"))]))
        .await
        .expect("patch in the set must succeed");
    assert!(has(&patched, "name", &t("p")) && has(&patched, "parent", &t("p1")));
    let e = fails(
        f.set
            .patch_value((f.id)("out1"), &rec(vec![("name", t("p"))]))
            .await,
        "patch of a row outside the set must fail",
    );
    assert!(e.is_not_found(), "patch outside the set is NotFound: {e}");
    assert!(
        has(&row(f.all, (f.id)("out1")).await.unwrap(), "name", &t("b")),
        "the outside row is untouched"
    );
    let e = fails(
        f.set
            .patch_value((f.id)("ghost"), &rec(vec![("name", t("p"))]))
            .await,
        "patch of a missing row must fail",
    );
    assert!(e.is_not_found(), "{e}");
    let e = fails(
        f.set
            .patch_value((f.id)("in1"), &rec(vec![("parent", t("p2"))]))
            .await,
        "a patch that leaves the set must fail",
    );
    assert!(e.is_conflict(), "{e}");
    assert!(
        has(
            &row(f.all, (f.id)("in1")).await.unwrap(),
            "parent",
            &t("p1")
        ),
        "the row stays in the set"
    );
}

pub async fn check_replace<S>(f: &Fixture<'_, S>)
where
    S: ReadableValueSet + WritableValueSet + Sync,
    S::Value: InvariantValue,
{
    let t = f.text;
    f.set
        .replace_value((f.id)("in1"), &rec(vec![("name", t("r"))]))
        .await
        .expect("replace in the set");
    let r = row(f.all, (f.id)("in1")).await.unwrap();
    assert!(
        has(&r, "name", &t("r")) && has(&r, "parent", &t("p1")),
        "replace fills the set's equality"
    );
    let outside = f
        .set
        .replace_value((f.id)("out1"), &rec(vec![("name", t("r"))]))
        .await;
    if f.detects_outside {
        assert!(fails(outside, "replace outside the set must fail").is_conflict());
    }
    assert!(
        has(
            &row(f.all, (f.id)("out1")).await.unwrap(),
            "parent",
            &t("p2")
        ),
        "outside row untouched"
    );
    f.set
        .replace_value((f.id)("new3"), &rec(vec![("name", t("r"))]))
        .await
        .expect("replace creates");
    assert!(has(
        &row(f.all, (f.id)("new3")).await.unwrap(),
        "parent",
        &t("p1")
    ));
}

pub async fn check_delete_all<S>(f: &Fixture<'_, S>)
where
    S: ReadableValueSet + WritableValueSet + Sync,
    S::Value: InvariantValue,
{
    f.set.delete_all().await.expect("delete_all");
    assert!(row(f.all, (f.id)("in1")).await.is_none());
    assert!(
        row(f.all, (f.id)("out1")).await.is_some(),
        "delete_all must keep rows outside the set"
    );
}

pub async fn check_operator_condition<S>(f: &OpFixture<'_, S>)
where
    S: ReadableValueSet + WritableValueSet + Sync,
    S::Value: InvariantValue,
{
    f.set
        .delete((f.id)("cheap"))
        .await
        .expect("delete outside the set reports success");
    assert!(
        row(f.all, (f.id)("cheap")).await.is_some(),
        "a row failing the operator stays"
    );
    let e = fails(
        f.set
            .insert_value(
                (f.id)("tiny"),
                &rec(vec![("name", (f.text)("t")), ("price", (f.int)(3))]),
            )
            .await,
        "an insert failing the operator condition must fail",
    );
    assert!(e.is_conflict(), "{e}");
    assert!(row(f.all, (f.id)("tiny")).await.is_none());
    let e = fails(
        f.set
            .patch_value((f.id)("dear"), &rec(vec![("price", (f.int)(1))]))
            .await,
        "a patch that fails the operator condition must fail",
    );
    assert!(e.is_conflict(), "{e}");
    assert!(has(
        &row(f.all, (f.id)("dear")).await.unwrap(),
        "price",
        &(f.int)(20)
    ));
    f.set
        .insert_value(
            (f.id)("big"),
            &rec(vec![("name", (f.text)("b")), ("price", (f.int)(50))]),
        )
        .await
        .expect("an insert that fits the operator condition succeeds");
}

/// `set` holds a condition the backend can neither apply nor evaluate:
/// every by-id write is refused with `Unsupported`.
pub async fn check_refused<S>(set: &S, id: fn(&str) -> S::Id, text: fn(&str) -> S::Value)
where
    S: ReadableValueSet + WritableValueSet + Sync,
{
    let r = rec(vec![("name", text("x"))]);
    for e in [
        set.insert_value(id("z"), &r).await.err(),
        set.replace_value(id("z"), &r).await.err(),
        set.patch_value(id("z"), &r).await.err(),
        set.delete(id("z")).await.err(),
    ] {
        let e = e.expect("a write through an unconfinable set must be refused");
        assert!(e.is_unsupported(), "refusal must be Unsupported: {e}");
    }
}
