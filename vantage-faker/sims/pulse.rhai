// builtin:pulse — a single sim keeping a live aggregate per key, a feed of
// its recent changes and per-bucket totals of the gains.
//
// Args and defaults:
//   tables:  aggregate: "top", feed: "updates", minutes: "minutes"
//   columns: key_column: "key", value_column: "value"
//   keys: [] (each #{name, baseline}), band: 5.0 (percent)
//   timing (seconds): min_interval: 1.0, max_interval: 3.0,
//                     feed_retention: 10.0, bucket: 60.0
//   window: 10 (buckets kept), offline: [] (key names that drop out)
//
// State: per key, `value` (starts at baseline) and `next_at` (sim secs);
// feed entries as [id, expires_at] in an array.
//
// Each wake:
//   1. For each due key: if the key is in `offline` and (now % 51) >= 35,
//      upsert its aggregate with live: "Offline" and the value unchanged.
//      Otherwise:
//      - delta = rand_int(-step, step), step = max(1, baseline * band / 100 / 4);
//      - value += delta, clamped to baseline ± band% (bounds rounded inward
//        when the baseline is an integer, so the value stays an integer);
//      - table(aggregate).upsert(key, #{key_column: key, value_column: value,
//          vs_baseline: round((value - baseline) / baseline * 100), live: "Live"});
//      - upsert a feed row #{key_column: key, delta, updated: <iso now>}
//        at the descending id, remembering its expiry;
//      - if delta > 0, add it into the current bucket
//        minute = floor(now / bucket) * bucket (id = pad(minute, 12),
//        row #{minute: <HH:MM>, attendees}) via get, then upsert.
//      Either way, next_at = now + rand_float(min_interval, max_interval).
//   2. Delete expired feed rows.
//   3. Delete buckets older than `window` buckets.
//   4. sleep until the earliest next_at (at least 0.1 s).
//
// Feed ids are pad(10_000_000_000_000 - ms, 13) + "-" + key, so sorting them
// ascending lists the newest change first. The feed and minutes tables need
// their own id column (not `key_column` or `minute`): upsert writes the
// row's id into whatever column the table names as its id, so pointing
// that at a data column overwrites it with the id.
//
// Throws at start on an empty `keys` list, or a key whose `baseline` is 0
// (it divides vs_baseline).
//
// Suggested def: spawn: { burst: 1, max: 1 }

// `n` as a decimal string, left-padded with zeros to `width` characters.
fn pad(n, width) {
    let s = n.to_string();
    while s.len() < width {
        s = "0" + s;
    }
    s
}

// Arg `name` if given, else `fallback`.
fn arg(args, name, fallback) {
    let v = args[name];
    if v == () { fallback } else { v }
}

// `x` as an integer when it has no fraction, so whole counts stay integers.
fn tidy(x) {
    if x == x.floor() { x.to_int() } else { x }
}

// Sim unix seconds `secs` as a UTC "HH:MM".
fn hh_mm(secs) {
    let s = secs.to_int() % 86400;
    pad(s / 3600, 2) + ":" + pad((s % 3600) / 60, 2)
}

let aggregate = arg(args, "aggregate", "top");
let feed_table = arg(args, "feed", "updates");
let minutes_table = arg(args, "minutes", "minutes");
let key_column = arg(args, "key_column", "key");
let value_column = arg(args, "value_column", "value");
let band = 0.0 + arg(args, "band", 5.0);
let min_interval = 0.0 + arg(args, "min_interval", 1.0);
let max_interval = 0.0 + arg(args, "max_interval", 3.0);
let feed_retention = 0.0 + arg(args, "feed_retention", 10.0);
let bucket = 0.0 + arg(args, "bucket", 60.0);
let window = arg(args, "window", 10);
let offline = arg(args, "offline", []);

// Every key is due on the first wake.
let raw_keys = arg(args, "keys", []);
if raw_keys.len() == 0 {
    throw "pulse: keys must not be empty";
}
let state = [];
for k in raw_keys {
    let baseline = 0.0 + k.baseline;
    if baseline == 0.0 {
        throw "pulse: key " + k.name + ": baseline must not be 0";
    }
    state.push(#{ name: k.name, baseline: baseline, value: baseline, next_at: now_secs() });
}

let feed = [];     // [id, expires_at], oldest first
let buckets = [];  // bucket ids, oldest first

loop {
    let now = now_secs();

    for i in 0..state.len() {
        let s = state[i];
        if s.next_at > now {
            continue;
        }
        let dropped = offline.contains(s.name) && (now % 51) >= 35;
        let delta = 0;
        if !dropped {
            let step = (s.baseline * band / 100.0 / 4.0).floor().to_int();
            if step < 1 { step = 1; }
            delta = rand_int(-step, step);
            let spread = s.baseline * band / 100.0;
            let lo = s.baseline - spread;
            let hi = s.baseline + spread;
            if s.baseline == s.baseline.floor() {
                lo = lo.ceiling();
                hi = hi.floor();
            }
            s.value = (s.value + delta).max(lo).min(hi);
        }

        let agg = #{
            vs_baseline: ((s.value - s.baseline) / s.baseline * 100.0).round().to_int(),
            live: if dropped { "Offline" } else { "Live" },
        };
        agg[key_column] = s.name;
        agg[value_column] = tidy(s.value);
        table(aggregate).upsert(s.name, agg);

        if !dropped {
            let ms = (now * 1000.0).to_int();
            let id = pad(10_000_000_000_000 - ms, 13) + "-" + s.name;
            let entry = #{ delta: delta, updated: now() };
            entry[key_column] = s.name;
            table(feed_table).upsert(id, entry);
            feed.push([id, now + feed_retention]);

            if delta > 0 {
                let minute = (now / bucket).floor() * bucket;
                let bid = pad(minute.to_int(), 12);
                let row = table(minutes_table).get(bid);
                if row == () {
                    row = #{ minute: hh_mm(minute), attendees: 0 };
                    buckets.push(bid);
                }
                row.attendees += delta;
                table(minutes_table).upsert(bid, row);
            }
        }

        s.next_at = now + rand_float(min_interval, max_interval);
        state[i] = s;
    }

    while feed.len() > 0 && feed[0][1] <= now {
        table(feed_table).delete(feed[0][0]);
        feed.remove(0);
    }

    while buckets.len() > window {
        table(minutes_table).delete(buckets[0]);
        buckets.remove(0);
    }

    let next = now + 3600.0;
    for s in state {
        if s.next_at < next { next = s.next_at; }
    }
    sleep((next - now).max(0.1));
}
