//! Time, control, spawn and geo verbs.

use vantage_rhai::rhai::{Dynamic, Engine, Map as RhaiMap};

use super::num;
use crate::generator::{parse_when, rfc3339};
use crate::sim::current::{Current, VerbResult, terminate, with};
use crate::sim::spawn::spawn_sim;

fn stamp(secs: f64) -> String {
    rfc3339(secs.round() as i64)
}

fn duration(name: &str, n: &Dynamic, unit: f64) -> VerbResult<f64> {
    let n = num(n)?;
    if n.is_finite() {
        Ok(n * unit)
    } else {
        Err(format!("{name}: {n} is not a finite duration").into())
    }
}

fn spawn(c: &mut Current, name: &str, args: RhaiMap) -> VerbResult<bool> {
    let Some(&kind) = c.inner.by_name.get(name) else {
        return Err(format!("spawn: no sim named {name}").into());
    };
    // Same instant, on the child's clock.
    let wall = c.kind().clock.wall(c.vt);
    let vt = c.inner.kinds[kind].clock.sim(wall);
    Ok(spawn_sim(&c.inner, kind, vt, args))
}

pub(super) fn register(engine: &mut Engine) {
    for (name, unit) in [
        ("seconds", 1.0),
        ("minutes", 60.0),
        ("hours", 3600.0),
        ("days", 86_400.0),
    ] {
        engine.register_fn(name, move |n: Dynamic| duration(name, &n, unit));
    }

    // Typed overloads: they must shadow Rhai's built-in `sleep(float)`,
    // which blocks on real time.
    fn sleep(d: Dynamic) -> VerbResult<()> {
        with(|c| {
            let d = duration("sleep", &d, 1.0)?.max(0.0);
            c.sleep_until(c.vt + d)
        })
    }
    engine.register_fn("sleep", |d: f64| sleep(Dynamic::from(d)));
    engine.register_fn("sleep", |d: i64| sleep(Dynamic::from(d)));
    engine.register_fn("wait_until", |t: Dynamic| {
        with(|c| {
            let target = match t.clone().into_string() {
                Ok(s) => parse_when(&s, c.vt as i64)
                    .ok_or_else(|| format!("wait_until: cannot read time {s:?}"))?
                    as f64,
                Err(_) => num(&t)?,
            };
            c.sleep_until(target)
        })
    });
    engine.register_fn("done", || -> VerbResult<()> {
        with(|c| {
            c.done = true;
            Err(terminate("done"))
        })
    });

    engine.register_fn("now", || with(|c| Ok(stamp(c.vt))));
    engine.register_fn("now_secs", || with(|c| Ok(c.vt)));
    engine.register_fn("wall_now", || {
        with(|c| Ok(stamp(c.kind().clock.wall(c.vt))))
    });
    engine.register_fn("wall_in", |d: Dynamic| {
        with(|c| {
            let d = duration("wall_in", &d, 1.0)?;
            Ok(stamp(c.kind().clock.wall(c.vt + d)))
        })
    });
    engine.register_fn("elapsed", || with(|c| Ok(c.vt - c.started)));
    engine.register_fn("clock", || with(|c| Ok(c.kind().clock.scale)));

    // `spawn` is a reserved word in Rhai.
    engine.register_fn("spawn_sim", |name: &str| {
        with(|c| spawn(c, name, RhaiMap::new()))
    });
    engine.register_fn("spawn_sim", |name: &str, args: RhaiMap| {
        with(|c| spawn(c, name, args))
    });
    engine.register_fn("sim_id", || with(|c| Ok(c.id as i64)));
    engine.register_fn("sim_name", || with(|c| Ok(c.kind().def.name.clone())));

    engine.register_fn(
        "great_circle",
        |lat1: Dynamic, lon1: Dynamic, lat2: Dynamic, lon2: Dynamic| -> VerbResult<f64> {
            Ok(crate::geo::distance_km(
                num(&lat1)?,
                num(&lon1)?,
                num(&lat2)?,
                num(&lon2)?,
            ))
        },
    );
    engine.register_fn(
        "bearing",
        |lat1: Dynamic, lon1: Dynamic, lat2: Dynamic, lon2: Dynamic| -> VerbResult<f64> {
            Ok(crate::geo::bearing(
                num(&lat1)?,
                num(&lon1)?,
                num(&lat2)?,
                num(&lon2)?,
            ))
        },
    );
    engine.register_fn(
        "interpolate",
        |lat1: Dynamic,
         lon1: Dynamic,
         lat2: Dynamic,
         lon2: Dynamic,
         f: Dynamic|
         -> VerbResult<RhaiMap> {
            let f = num(&f)?.clamp(0.0, 1.0);
            let (lat, lon) =
                crate::geo::interpolate(num(&lat1)?, num(&lon1)?, num(&lat2)?, num(&lon2)?, f);
            let mut map = RhaiMap::new();
            map.insert("lat".into(), Dynamic::from(lat));
            map.insert("lon".into(), Dynamic::from(lon));
            Ok(map)
        },
    );
}
