// builtin:fifo — one sim per row: a row arrives, stays a while, and leaves.
//
// Args:
//   retention_lo  number, seconds, default 20
//   retention_hi  number, seconds, default 35
//
// Id: pad(10_000_000_000_000 - ms, 13) + "-" + pad(1_000_000_000 - sim_id(), 9),
//     where ms = (now_secs() * 1000.0).to_int()
// The id counts down with time, so sorting ids ascending lists the newest
// row first. The sim id component also counts down and is fixed-width, so
// two arrivals in the same millisecond still sort newest (highest sim id)
// first regardless of how many digits either sim id has.
//
// Steps:
//   1. let r = table().fake_row(); table().upsert(<id>, r);
//   2. sleep(seconds(rand_float(lo, hi)))
//   3. table().delete(id)
//
// `upsert` writes the id into the table's id column, whatever it is named.
//
// Suggested def: spawn: { burst: 0, rate: <arrivals per sim minute>, max: 200 }
// The table's declared columns fill the row, so the def's table should list
// the columns each arrival carries.
//
// A non-number retention_lo / retention_hi throws
// "fifo: retention_lo must be a number of seconds".

// `n` as a decimal string, left-padded with zeros to `width` characters.
fn pad(n, width) {
    let s = n.to_string();
    while s.len() < width {
        s = "0" + s;
    }
    s
}

// Arg `name` if given, else `fallback`; throws unless it is a number.
fn seconds_arg(args, name, fallback) {
    let v = args[name];
    if v == () {
        return fallback;
    }
    if type_of(v) != "i64" && type_of(v) != "f64" {
        throw "fifo: " + name + " must be a number of seconds";
    }
    v
}

let lo = seconds_arg(args, "retention_lo", 20);
let hi = seconds_arg(args, "retention_hi", 35);

let ms = (now_secs() * 1000.0).to_int();
let id = pad(10_000_000_000_000 - ms, 13) + "-" + pad(1_000_000_000 - sim_id(), 9);

let r = table().fake_row();
table().upsert(id, r);

sleep(seconds(rand_float(lo, hi)));

table().delete(id);
