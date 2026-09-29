// builtin:folder_tree — a single sim growing a live folder of log files.
//
// Args:
//   requests_per_sec   number, default 100
//   bytes_per_request  [lo, hi] or a number, bytes per access-log line,
//                      default [60, 100]
//   chunk_threshold    number, bytes an access chunk grows to before the
//                      next one opens, default 288000 (about 100 chunks/hour)
//   error_pct          number, percent chance per second of an error file,
//                      default 1.0
//
// Writes the def's table (suggested `nodes`, id column `path`, `parent`
// indexed), one row per folder or file:
//   #{ path, parent, name, kind: "folder" | "file", size, created, modified }
// `path` is "/"-separated; the day folder is the root, with parent "".
// `created` / `modified` are sim time, RFC 3339. Listing a folder means
// filtering on `parent == <path>`.
//
// The tree, per UTC day:
//   {date}/access_logs_{HH}/chunk_{NN}.log
//   {date}/error_logs/{HH:MM:SS}-errors.log
//   {date}/events/{event_type}.log
//
// Each sim second:
//   1. The active chunk of this hour grows by requests_per_sec × one draw
//      of bytes_per_request. Once it reaches chunk_threshold, the next chunk
//      opens at size 0 and takes the following growth.
//   2. With error_pct chance, an error file of 500-3000 bytes appears.
//   3. Each event type fires on its own chance and grows its file by
//      2000-4000 bytes (the file appears on its first event).
//   4. Every folder above a touched file gets modified = now and size = the
//      sum of its children's sizes, deepest first.
//
// Suggested def: table: nodes, warm: 1h (the backfill),
//   spawn: { burst: 1, max: 1 }

// Arg `name` if given, else `fallback`.
fn arg(args, name, fallback) {
    let v = args[name];
    if v == () { fallback } else { v }
}

// `n` as a decimal string, left-padded with zeros to `width` characters.
fn pad(n, width) {
    let s = n.to_string();
    while s.len() < width {
        s = "0" + s;
    }
    s
}

// Create folder `path` under `parent` unless it exists.
fn ensure_folder(path, parent, name, ts) {
    if get(path) == () {
        upsert(path, #{
            path: path, parent: parent, name: name, kind: "folder",
            size: 0, created: ts, modified: ts,
        });
    }
}

// Grow file `name` in folder `dir` by `bytes` (creating it at `bytes`) and
// return its new size.
fn grow(dir, name, bytes, ts) {
    let path = dir + "/" + name;
    let old = get(path);
    let size = bytes;
    let created = ts;
    if old != () {
        size += old.size;
        created = old.created;
    }
    upsert(path, #{
        path: path, parent: dir, name: name, kind: "file",
        size: size, created: created, modified: ts,
    });
    size
}

// Set folder `path`'s size to the sum of its children's and touch it.
fn refresh(path, ts) {
    let total = 0;
    for child in find(#{ parent: path }) {
        total += get(child).size;
    }
    patch(path, #{ size: total, modified: ts });
}

// [event type, percent chance per second]
let event_types = [
    ["user_signup", 1], ["payment_succeeded", 2], ["payment_failed", 1],
    ["page_view", 10], ["api_call", 8], ["search_query", 6],
    ["file_upload", 3], ["email_sent", 4], ["notification", 5],
    ["error_reported", 1],
];

let requests_per_sec = arg(args, "requests_per_sec", 100);
let per_request = arg(args, "bytes_per_request", [60, 100]);
if type_of(per_request) != "array" {
    per_request = [per_request, per_request];
}
let chunk_threshold = arg(args, "chunk_threshold", 288000);
let error_pct = 0.0 + arg(args, "error_pct", 1.0);

// The active chunk: its folder and number.
let chunk_dir = "";
let chunk_no = 1;

loop {
    let ts = now();
    let date = ts.sub_string(0, 10);
    let hms = ts.sub_string(11, 8);
    ensure_folder(date, "", date, ts);
    let touched = [];

    let access = date + "/access_logs_" + ts.sub_string(11, 2);
    if access != chunk_dir {
        ensure_folder(access, date, "access_logs_" + ts.sub_string(11, 2), ts);
        chunk_dir = access;
        chunk_no = 1;
    }
    let bytes = rand_int(per_request[0], per_request[1]) * requests_per_sec;
    if grow(access, "chunk_" + pad(chunk_no, 2) + ".log", bytes, ts) >= chunk_threshold {
        chunk_no += 1;
        grow(access, "chunk_" + pad(chunk_no, 2) + ".log", 0, ts);
    }
    touched.push(access);

    if chance(error_pct / 100.0) {
        let errors = date + "/error_logs";
        ensure_folder(errors, date, "error_logs", ts);
        grow(errors, hms + "-errors.log", rand_int(500, 3000), ts);
        touched.push(errors);
    }

    let events = date + "/events";
    for e in event_types {
        if chance(e[1] / 100.0) {
            ensure_folder(events, date, "events", ts);
            grow(events, e[0] + ".log", rand_int(2000, 4000), ts);
            if !touched.contains(events) {
                touched.push(events);
            }
        }
    }

    // Every touched folder sits directly under the day folder.
    for dir in touched {
        refresh(dir, ts);
    }
    refresh(date, ts);

    sleep(seconds(1));
}
