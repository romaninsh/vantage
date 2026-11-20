//! Dates, times and instants reach the store and come back in the carrier forms of
//! [`vantage_types::temporal`], whatever form they were written in and whoever wrote them.

use async_trait::async_trait;
use vantage_core::{Result, error};
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_types::Record;
use vantage_types::temporal::{
    TAG_DATE, TAG_INSTANT, TAG_LEGACY_DATE, TAG_SURREAL_INSTANT, TAG_TIME,
};

use super::transactions::{PROBE_DEADLINE, Runner, ensure, run};
use crate::{Capability, CborValue, FilterOp, SortDirection, Vista};

type Rec = Record<CborValue>;

/// What the store holds for one cell, as the fixture reads it past the vista.
#[derive(Debug, Clone, PartialEq)]
pub enum Stored {
    /// The store's own temporal type (`kind`, e.g. `timestamp without time zone`, BSON `date`),
    /// rendered by the fixture in the carrier's text form: an instant as its UTC wall clock with
    /// six fraction digits and `Z`, a date as `YYYY-MM-DD`, a time as `HH:MM:SS.ffffff`.
    Native { kind: String, canonical: String },
    /// Text, exactly as stored.
    Text(String),
    /// A carrier value, exactly as stored (memory, redb).
    Carrier(CborValue),
}

/// Sub-second digits an instant or a time keeps in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    Micros,
    Millis,
}

/// A row written straight to the store, as another program would, and what it must read as.
#[non_exhaustive]
pub struct Foreign {
    pub column: &'static str,
    /// What the store was given, for the failure message.
    pub written: String,
    /// The carrier value a read must return.
    pub reads_as: CborValue,
}

impl Foreign {
    pub fn new(column: &'static str, written: impl Into<String>, reads_as: CborValue) -> Self {
        Self {
            column,
            written: written.into(),
            reads_as,
        }
    }
}

#[async_trait]
pub trait TemporalFixture: Send + Sync {
    /// A new, empty table with a text id and the columns `d` (date), `t` (time) and `ts`
    /// (timestamp), plus `tz` when [`zone_aware`](Self::zone_aware) is true.
    async fn fresh(&self) -> Vista;
    /// Whether the table also has `tz`, a column of the store's zone-aware timestamp type.
    fn zone_aware(&self) -> bool {
        false
    }
    /// Whether `tz` holds the whole edge range (1969 to 9999). MySQL's `TIMESTAMP` stops at
    /// 1970–2038, so its fixture says no and the edge probe writes `tz` only in that range.
    fn zone_aware_full_range(&self) -> bool {
        true
    }
    fn precision(&self) -> Precision {
        Precision::Micros
    }
    /// The cell `column` of row `id`, read past the vista.
    async fn stored(&self, vista: &Vista, id: &str, column: &str) -> Result<Stored>;
    /// Writes rows straight to the store in every form it may already hold, and says what each
    /// must read as. Each row's id is its index in the returned list (`"f0"`, `"f1"`, …).
    async fn seed_foreign(&self, vista: &Vista) -> Result<Vec<Foreign>>;
}

/// Runs every temporal probe on fresh tables, then panics once, listing every broken rule. A
/// table that takes no inserts gets only the probe that reads rows written past it.
pub async fn check_temporal(fixture: &impl TemporalFixture) {
    let mut runner = Runner::new();
    let fx = fixture;
    if !fx.fresh().await.capabilities().has(Capability::Insert) {
        run(
            &mut runner,
            "foreign_rows_read_canonical",
            PROBE_DEADLINE,
            foreign_rows_read_canonical(fx),
        )
        .await;
        return runner.finish();
    }
    run(
        &mut runner,
        "canonical_round_trips",
        PROBE_DEADLINE,
        canonical_round_trips(fx),
    )
    .await;
    run(
        &mut runner,
        "accepted_inputs_normalise",
        PROBE_DEADLINE,
        accepted_inputs_normalise(fx),
    )
    .await;
    run(
        &mut runner,
        "storage_is_the_drivers",
        PROBE_DEADLINE,
        storage_is_the_drivers(fx),
    )
    .await;
    run(
        &mut runner,
        "foreign_rows_read_canonical",
        PROBE_DEADLINE,
        foreign_rows_read_canonical(fx),
    )
    .await;
    run(
        &mut runner,
        "precision_is_kept",
        PROBE_DEADLINE,
        precision_is_kept(fx),
    )
    .await;
    run(
        &mut runner,
        "edges_round_trip",
        PROBE_DEADLINE,
        edges_round_trip(fx),
    )
    .await;
    run(&mut runner, "null_clears", PROBE_DEADLINE, null_clears(fx)).await;
    run(
        &mut runner,
        "conditions_compare_values",
        PROBE_DEADLINE,
        conditions_compare_values(fx),
    )
    .await;
    run(
        &mut runner,
        "order_is_chronological",
        PROBE_DEADLINE,
        order_is_chronological(fx),
    )
    .await;
    runner.finish();
}

const TS: &str = "2026-11-19T16:01:00.000000Z";
const DAY: &str = "2026-11-19";
const TIME: &str = "16:01:00.000000";

async fn canonical_round_trips(fx: &impl TemporalFixture) -> Result<()> {
    let v = fx.fresh().await;
    let row = row(fx, &[("d", date(DAY)), ("t", time(TIME))], instant(TS));
    v.insert_value("r", &row).await?;
    expect_row(fx, &v, "r", &row, "written in the carrier form").await
}

async fn accepted_inputs_normalise(fx: &impl TemporalFixture) -> Result<()> {
    let v = fx.fresh().await;
    let half = "2026-11-19T16:01:00.500000Z";
    let cases: Vec<(&str, CborValue, CborValue)> = vec![
        ("d", text(DAY), date(DAY)),
        ("d", tagged(TAG_LEGACY_DATE, DAY), date(DAY)),
        ("d", tagged(TAG_INSTANT, "2026-11-19T00:00:00Z"), date(DAY)),
        ("t", text("16:01"), time(TIME)),
        ("t", tagged(TAG_TIME, "16:01:00.5"), time("16:01:00.500000")),
        ("ts", text("2026-11-19T18:01:00.5+02:00"), instant(half)),
        (
            "ts",
            tagged(TAG_INSTANT, "2026-11-19T18:01:00.5+02:00"),
            instant(half),
        ),
        ("ts", text("2026-11-19 16:01:00.5"), instant(half)),
        (
            "ts",
            tagged(TAG_INSTANT, "2026-11-19T16:01:00.5Z"),
            instant(half),
        ),
        ("ts", surreal(1_795_104_060, 500_000_000), instant(half)),
    ];
    let mut broken = Vec::new();
    for (i, (column, given, wanted)) in cases.into_iter().enumerate() {
        let id = format!("a{i}");
        let wanted = truncate(fx.precision(), wanted);
        match v.insert_value(&id, &one(column, given.clone())).await {
            Err(e) => broken.push(format!("{column} = {given:?} is refused: {e}")),
            Ok(_) => {
                let got = cell(&v, &id, column).await?;
                if got != wanted {
                    broken.push(format!(
                        "{column} = {given:?} reads {got:?}, not {wanted:?}"
                    ));
                }
            }
        }
    }
    all_held(broken)
}

async fn storage_is_the_drivers(fx: &impl TemporalFixture) -> Result<()> {
    let v = fx.fresh().await;
    let row = row(fx, &[("d", date(DAY)), ("t", time(TIME))], instant(TS));
    v.insert_value("s", &row).await?;
    let mut broken = Vec::new();
    for (column, value) in row.iter() {
        let stored = fx.stored(&v, "s", column).await?;
        let CborValue::Tag(_, inner) = value else {
            continue;
        };
        let CborValue::Text(canonical) = &**inner else {
            continue;
        };
        let holds = match &stored {
            Stored::Native { canonical: c, .. } => c == canonical,
            Stored::Text(t) => t == canonical,
            Stored::Carrier(c) => c == value,
        };
        if !holds {
            broken.push(format!(
                "{column} written as {value:?} is stored as {stored:?}"
            ));
        }
    }
    all_held(broken)
}

async fn foreign_rows_read_canonical(fx: &impl TemporalFixture) -> Result<()> {
    let v = fx.fresh().await;
    let seeded = fx.seed_foreign(&v).await?;
    ensure(
        !seeded.is_empty(),
        "the fixture seeds no foreign rows".to_string(),
    )?;
    let mut broken = Vec::new();
    for (i, f) in seeded.iter().enumerate() {
        let id = format!("f{i}");
        match v.get_value(&id).await {
            Err(e) => broken.push(format!(
                "{} stored as {} fails to read: {e}",
                f.column, f.written
            )),
            Ok(None) => broken.push(format!(
                "row {id} ({} stored as {}) is not found",
                f.column, f.written
            )),
            Ok(Some(row)) => {
                let got = row.get(f.column).cloned().unwrap_or(CborValue::Null);
                if got != f.reads_as {
                    broken.push(format!(
                        "{} stored as {} reads {got:?}, not {:?}",
                        f.column, f.written, f.reads_as
                    ));
                }
            }
        }
    }
    all_held(broken)
}

async fn precision_is_kept(fx: &impl TemporalFixture) -> Result<()> {
    let v = fx.fresh().await;
    let row = row(
        fx,
        &[("t", time("16:01:00.123456"))],
        instant("2026-11-19T16:01:00.123456Z"),
    );
    v.insert_value("p", &row).await?;
    let wanted: Rec = row
        .iter()
        .map(|(k, val)| (k.clone(), truncate(fx.precision(), val.clone())))
        .collect();
    expect_row(fx, &v, "p", &wanted, "with microseconds").await
}

async fn edges_round_trip(fx: &impl TemporalFixture) -> Result<()> {
    let v = fx.fresh().await;
    let cases = [
        (
            "e0",
            "1969-07-20",
            "20:17:40.000000",
            "1969-07-20T20:17:40.000000Z",
        ),
        (
            "e1",
            "2024-02-29",
            "23:59:59.999999",
            "2024-02-29T23:59:59.999999Z",
        ),
        (
            "e2",
            "9999-12-31",
            "00:00:00.000000",
            "9999-12-31T23:59:59.999999Z",
        ),
    ];
    let mut broken = Vec::new();
    for (id, d, t, ts) in cases {
        let mut row = row(fx, &[("d", date(d)), ("t", time(t))], instant(ts));
        if id != "e1" && !fx.zone_aware_full_range() {
            row.shift_remove("tz");
        }
        if let Err(e) = v.insert_value(id, &row).await {
            broken.push(format!("{d} / {t} / {ts} is refused: {e}"));
            continue;
        }
        if let Err(e) = expect_row(fx, &v, id, &row, "at an edge").await {
            broken.push(e.to_string());
        }
    }
    all_held(broken)
}

async fn null_clears(fx: &impl TemporalFixture) -> Result<()> {
    let v = fx.fresh().await;
    let row = row(fx, &[("d", date(DAY)), ("t", time(TIME))], instant(TS));
    v.insert_value("n", &row).await?;
    let cleared: Rec = row.keys().map(|k| (k.clone(), CborValue::Null)).collect();
    v.patch_value("n", &cleared).await?;
    let got = v
        .get_value("n")
        .await?
        .ok_or_else(|| error!("n is not found"))?;
    let left: Vec<String> = row
        .keys()
        .filter(|k| !matches!(got.get(*k), None | Some(CborValue::Null)))
        .map(|k| format!("{k} = {:?}", got.get(k)))
        .collect();
    ensure(
        left.is_empty(),
        format!("after patching every column to null: {}", left.join(", ")),
    )
}

/// Three rows a day, a minute and a fraction apart, so a text comparison of mixed forms and an
/// instant comparison disagree.
const ROWS: [(&str, &str, &str); 3] = [
    (
        "2026-11-18",
        "09:30:00.000000",
        "2026-11-18T09:30:00.000000Z",
    ),
    (
        "2026-11-19",
        "16:01:00.000000",
        "2026-11-19T16:01:00.000000Z",
    ),
    (
        "2026-11-20",
        "16:01:00.500000",
        "2026-11-19T16:01:00.500000Z",
    ),
];

async fn three_rows(fx: &impl TemporalFixture) -> Result<Vista> {
    let v = fx.fresh().await;
    for (i, (d, t, ts)) in ROWS.iter().enumerate() {
        v.insert_value(
            format!("c{i}"),
            &row(fx, &[("d", date(d)), ("t", time(t))], instant(ts)),
        )
        .await?;
    }
    Ok(v)
}

async fn conditions_compare_values(fx: &impl TemporalFixture) -> Result<()> {
    let v = three_rows(fx).await?;
    let ts = |i: usize| instant(ROWS[i].2);
    let d = |i: usize| date(ROWS[i].0);
    let t = |i: usize| time(ROWS[i].1);
    // Each condition is given in a non-canonical form too, as a filter typed by a person would be.
    let cases: Vec<(&str, FilterOp, CborValue, Vec<usize>)> = vec![
        ("ts", FilterOp::Eq, ts(1), vec![1]),
        ("ts", FilterOp::Eq, text("2026-11-19 16:01:00"), vec![1]),
        ("ts", FilterOp::Gt, ts(1), vec![2]),
        (
            "ts",
            FilterOp::Lt,
            text("2026-11-19T17:01:00+01:00"),
            vec![0],
        ),
        (
            "ts",
            FilterOp::InSet,
            CborValue::Array(vec![ts(0), ts(2)]),
            vec![0, 2],
        ),
        ("d", FilterOp::Eq, d(1), vec![1]),
        ("d", FilterOp::Gte, text("2026-11-19"), vec![1, 2]),
        ("t", FilterOp::Eq, text("16:01"), vec![1]),
        ("t", FilterOp::Lt, t(1), vec![0]),
    ];
    let mut broken = Vec::new();
    for (column, op, value, wanted) in cases {
        let label = format!("{column} {op:?} {value:?}");
        let narrowed = match v.clone().with_condition(column, op, value) {
            Ok(n) => n,
            Err(e) => {
                broken.push(format!("{label} is refused: {e}"));
                continue;
            }
        };
        let mut got: Vec<CborValue> = match narrowed.list_values().await {
            Ok(rows) => rows
                .into_values()
                .map(|r| r.get("ts").cloned().unwrap_or(CborValue::Null))
                .collect(),
            Err(e) => {
                broken.push(format!("{label} fails to list: {e}"));
                continue;
            }
        };
        got.sort_by_key(|c| format!("{c:?}"));
        let mut want: Vec<CborValue> = wanted
            .iter()
            .map(|&i| truncate(fx.precision(), ts(i)))
            .collect();
        want.sort_by_key(|c| format!("{c:?}"));
        if got != want {
            broken.push(format!(
                "{label} selects rows with ts {got:?}, not {want:?}"
            ));
        }
    }
    all_held(broken)
}

async fn order_is_chronological(fx: &impl TemporalFixture) -> Result<()> {
    let v = three_rows(fx).await?;
    let mut broken = Vec::new();
    for (column, wanted) in [("ts", [0, 1, 2]), ("d", [0, 1, 2]), ("t", [0, 1, 2])] {
        let ordered = v.clone().with_order(column, SortDirection::Ascending)?;
        let got: Vec<CborValue> = ordered
            .list_values()
            .await?
            .into_values()
            .map(|r| r.get("ts").cloned().unwrap_or(CborValue::Null))
            .collect();
        let want: Vec<CborValue> = wanted
            .iter()
            .map(|&i| truncate(fx.precision(), instant(ROWS[i].2)))
            .collect();
        if got != want {
            broken.push(format!(
                "ordered by {column}, ts reads {got:?}, not {want:?}"
            ));
        }
    }
    all_held(broken)
}

/// A row with `extra` and `ts`, plus `tz` holding the same instant when the table has it.
fn row(fx: &impl TemporalFixture, extra: &[(&str, CborValue)], ts: CborValue) -> Rec {
    let mut row: Rec = extra
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    row.insert("ts".to_string(), ts.clone());
    if fx.zone_aware() {
        row.insert("tz".to_string(), ts);
    }
    row
}

fn one(column: &str, value: CborValue) -> Rec {
    [(column.to_string(), value)].into_iter().collect()
}

async fn expect_row(
    fx: &impl TemporalFixture,
    v: &Vista,
    id: &str,
    wanted: &Rec,
    how: &str,
) -> Result<()> {
    let got = v
        .get_value(id)
        .await?
        .ok_or_else(|| error!(format!("{id} is not found")))?;
    let broken: Vec<String> = wanted
        .iter()
        .filter_map(|(k, want)| {
            let want = truncate(fx.precision(), want.clone());
            let have = got.get(k).cloned().unwrap_or(CborValue::Null);
            (have != want).then(|| format!("{k} {how} as {want:?} reads {have:?}"))
        })
        .collect();
    all_held(broken)
}

async fn cell(v: &Vista, id: &str, column: &str) -> Result<CborValue> {
    let row = v
        .get_value(id)
        .await?
        .ok_or_else(|| error!(format!("{id} is not found")))?;
    Ok(row.get(column).cloned().unwrap_or(CborValue::Null))
}

fn all_held(broken: Vec<String>) -> Result<()> {
    ensure(broken.is_empty(), broken.join("; "))
}

/// An instant or a time cut to the store's precision; other values as they are.
fn truncate(precision: Precision, value: CborValue) -> CborValue {
    let Precision::Millis = precision else {
        return value;
    };
    match value {
        CborValue::Tag(tag @ (TAG_INSTANT | TAG_TIME), inner) => match *inner {
            CborValue::Text(s) => match s.find('.') {
                Some(dot) if s.len() >= dot + 7 => {
                    let cut = format!("{}000{}", &s[..dot + 4], &s[dot + 7..]);
                    CborValue::Tag(tag, Box::new(CborValue::Text(cut)))
                }
                _ => CborValue::Tag(tag, Box::new(CborValue::Text(s))),
            },
            other => CborValue::Tag(tag, Box::new(other)),
        },
        other => other,
    }
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn tagged(tag: u64, s: &str) -> CborValue {
    CborValue::Tag(tag, Box::new(text(s)))
}

fn instant(s: &str) -> CborValue {
    tagged(TAG_INSTANT, s)
}

fn date(s: &str) -> CborValue {
    tagged(TAG_DATE, s)
}

fn time(s: &str) -> CborValue {
    tagged(TAG_TIME, s)
}

fn surreal(secs: i64, nanos: i64) -> CborValue {
    CborValue::Tag(
        TAG_SURREAL_INSTANT,
        Box::new(CborValue::Array(vec![
            CborValue::Integer(secs.into()),
            CborValue::Integer(nanos.into()),
        ])),
    )
}
