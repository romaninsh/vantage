//! Rows over the fleet: projects flights onto the table's columns, patches
//! what changed, freezes landed rows and retires them after the retention.

use std::collections::HashMap;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_types::Record;

use super::columns::{flight_value, is_flight_column};
use super::flight::{Flight, Phase};
use super::sim::FlightSim;
use crate::value_gen::ValueGen;
use crate::{FakerColumn, FakerCtx};

struct Row {
    /// Sim-column values as last stored.
    sim: Record<CborValue>,
    /// Real time (since start) the row first showed `Landed`; frozen after.
    landed_at: Option<Duration>,
}

pub struct Board {
    pub sim: FlightSim,
    /// Sim-clock unix seconds at real elapsed zero.
    t0: f64,
    time_scale: f64,
    retention: Duration,
    columns: Vec<FakerColumn>,
    id_column: String,
    values: ValueGen,
    rows: HashMap<String, Row>,
}

impl Board {
    pub fn new(
        sim: FlightSim,
        t0: f64,
        time_scale: f64,
        retention: Duration,
        ctx: &FakerCtx,
        values: ValueGen,
    ) -> Self {
        Self {
            sim,
            t0,
            time_scale,
            retention,
            columns: ctx.columns().to_vec(),
            id_column: ctx.id_column().to_string(),
            values,
            rows: HashMap::new(),
        }
    }

    /// Sim time after `elapsed` of real time.
    pub fn sim_time(&self, elapsed: Duration) -> f64 {
        self.t0 + elapsed.as_secs_f64() * self.time_scale
    }

    /// Store every current flight without broadcasting.
    pub fn seed(&mut self, ctx: &FakerCtx) {
        let t = self.t0;
        let ids: Vec<String> = self.sim.flights.keys().cloned().collect();
        for id in ids {
            self.insert(ctx, &id, t, Duration::ZERO, false);
        }
    }

    /// Advance to `elapsed` real time: schedule new departures (inserted),
    /// patch rows whose values changed, delete landed rows past retention.
    pub fn advance(&mut self, ctx: &FakerCtx, elapsed: Duration) {
        let t = self.sim_time(elapsed);
        for id in self.sim.top_up(t) {
            self.insert(ctx, &id, t, elapsed, true);
        }

        let mut retired = Vec::new();
        for (id, row) in self.rows.iter_mut() {
            if let Some(at) = row.landed_at {
                if elapsed.saturating_sub(at) >= self.retention {
                    retired.push(id.clone());
                }
                continue;
            }
            let Some(flight) = self.sim.flights.get(id) else {
                continue;
            };
            let (now, landed) = project(&self.columns, flight, t);
            let changed: Record<CborValue> = now
                .iter()
                .filter(|(k, v)| row.sim.get(*k) != Some(*v))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if !changed.is_empty() {
                ctx.patch_record(id, &changed);
                row.sim = now;
            }
            if landed {
                row.landed_at = Some(elapsed);
            }
        }
        for id in retired {
            ctx.expire(&id);
            self.rows.remove(&id);
            self.sim.remove(&id);
        }
    }

    fn insert(&mut self, ctx: &FakerCtx, id: &str, t: f64, elapsed: Duration, broadcast: bool) {
        let flight = &self.sim.flights[id];
        let (sim, landed) = project(&self.columns, flight, t);
        let extras: Vec<FakerColumn> = self
            .columns
            .iter()
            .filter(|c| c.name != self.id_column && !is_flight_column(&c.name))
            .cloned()
            .collect();
        let generated = self
            .values
            .record_at(&extras, &self.id_column, id, flight.seq as usize);
        let mut record = Record::new();
        for col in &self.columns {
            let value = if col.name == self.id_column {
                CborValue::Text(id.to_string())
            } else if let Some(v) = sim.get(&col.name) {
                v.clone()
            } else {
                generated.get(&col.name).cloned().unwrap_or(CborValue::Null)
            };
            record.insert(col.name.clone(), value);
        }
        if !record.contains_key(&self.id_column) {
            record.insert(self.id_column.clone(), CborValue::Text(id.to_string()));
        }
        ctx.put_record(id, record, broadcast);
        let landed_at = landed.then_some(elapsed);
        self.rows.insert(id.to_string(), Row { sim, landed_at });
    }
}

/// The declared sim columns of `flight` at sim time `t`, and whether it has
/// landed.
fn project(columns: &[FakerColumn], flight: &Flight, t: f64) -> (Record<CborValue>, bool) {
    let state = flight.state_at(t);
    let record = columns
        .iter()
        .filter_map(|c| Some((c.name.clone(), flight_value(flight, &state, t, &c.name)?)))
        .collect();
    (record, state.phase == Phase::Landed)
}
