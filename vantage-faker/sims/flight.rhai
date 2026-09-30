// builtin:flight — one sim per aircraft: a flight from the departure board
// to the gate at the far end.
//
// Args:
//   landed_retention  number, sim seconds the Landed row stays, default 600
//
// Writes one row to the def's table, with these columns:
//   flight_no, carrier, aircraft_type, registration,
//   origin, origin_city, destination, destination_city, phase, progress (0-100),
//   altitude_ft, ground_speed_kts, heading, distance_total_nm,
//   distance_remaining_nm, scheduled_departure, actual_departure, eta,
//   flight_time ("11h 10m"), flight_time_min, delay_minutes, gate, pax,
//   seats, lat, lon, board_order.
// The row's id is an 8-digit sim id, written via `upsert` into the table's
// id column, whatever it is named. `patch`/`delete` below take the same id,
// so the phase updates and the eventual removal work the same way.
// Any other declared column of the table is filled once from row().
//
// Phases: Scheduled, Boarding (30 min before off-block), Taxiing (12 min),
// Takeoff, Climbing, Cruising, Descending, Landing, Landed. While airborne
// the row is patched every 30 sim seconds; the Landed row is deleted after
// `landed_retention`.
//
// The route touches the carrier's hub, the aircraft type has the range for
// it (widebodies stay off short hops), and the mix leans long-haul: routes
// of 3-16 h airborne, kept with chance hours / 16. Climb, cruise and descent
// are planned from the type's cruise speed and altitude; short routes climb
// lower.
//
// `scheduled_departure`, `actual_departure` and `eta` are wall-clock, so a UI
// can say "lands in 4 min"; `flight_time` is sim time. `board_order` is the
// scheduled departure in sim unix seconds, so sorting by it lists departures
// in time order.
//
// Suggested def (about 200 in the air):
//   clock: 10, warm: 12h, spawn: { burst: 0, rate: 0.3, max: 260 }

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

// `x` rounded to a multiple of `1 / k`.
fn round_by(x, k) {
    (x * k).round() / k
}

// A sim duration as "11h 10m", rounded to the minute.
fn hours_minutes(secs) {
    let m = (secs / 60.0).round().to_int();
    `${m / 60}h ${m % 60}m`
}

// [IATA, city, lat, lon]
fn airport_table() {
    [
        ["LHR", "London", 51.4700, -0.4543], ["CDG", "Paris", 49.0097, 2.5479], ["FRA", "Frankfurt", 50.0379, 8.5622],
        ["AMS", "Amsterdam", 52.3105, 4.7683], ["MAD", "Madrid", 40.4983, -3.5676], ["BCN", "Barcelona", 41.2974, 2.0833],
        ["FCO", "Rome", 41.8003, 12.2389], ["ZRH", "Zurich", 47.4582, 8.5555], ["CPH", "Copenhagen", 55.6180, 12.6508],
        ["DUB", "Dublin", 53.4264, -6.2499], ["IST", "Istanbul", 41.2753, 28.7519], ["CAI", "Cairo", 30.1219, 31.4056],
        ["JNB", "Johannesburg", -26.1367, 28.2411], ["DXB", "Dubai", 25.2532, 55.3657], ["DOH", "Doha", 25.2731, 51.6081],
        ["DEL", "Delhi", 28.5562, 77.1000], ["BOM", "Mumbai", 19.0896, 72.8656], ["SIN", "Singapore", 1.3644, 103.9915],
        ["HKG", "Hong Kong", 22.3080, 113.9185], ["PEK", "Beijing", 40.0799, 116.6031], ["PVG", "Shanghai", 31.1443, 121.8083],
        ["ICN", "Seoul", 37.4602, 126.4407], ["NRT", "Tokyo", 35.7720, 140.3929], ["SYD", "Sydney", -33.9399, 151.1753],
        ["JFK", "New York", 40.6413, -73.7781], ["ORD", "Chicago", 41.9742, -87.9073], ["ATL", "Atlanta", 33.6407, -84.4277],
        ["DFW", "Dallas", 32.8998, -97.0403], ["MIA", "Miami", 25.7959, -80.2870], ["YYZ", "Toronto", 43.6777, -79.6248],
        ["SEA", "Seattle", 47.4502, -122.3088], ["SFO", "San Francisco", 37.6213, -122.3790], ["LAX", "Los Angeles", 33.9416, -118.4085],
        ["MEX", "Mexico City", 19.4361, -99.0719], ["GRU", "Sao Paulo", -23.4356, -46.4731], ["EZE", "Buenos Aires", -34.8222, -58.5358],
        ["SCL", "Santiago", -33.3930, -70.7858], ["BOG", "Bogota", 4.7016, -74.1469], ["BOS", "Boston", 42.3656, -71.0096],
        ["IAD", "Washington", 38.9531, -77.4565], ["YVR", "Vancouver", 49.1967, -123.1815], ["HNL", "Honolulu", 21.3245, -157.9251],
        ["MEL", "Melbourne", -37.6690, 144.8410], ["PER", "Perth", -31.9385, 115.9672], ["AKL", "Auckland", -37.0082, 174.7850],
        ["BKK", "Bangkok", 13.6900, 100.7501], ["KUL", "Kuala Lumpur", 2.7456, 101.7072], ["CGK", "Jakarta", -6.1256, 106.6559],
        ["MNL", "Manila", 14.5086, 121.0194], ["TPE", "Taipei", 25.0797, 121.2342], ["NBO", "Nairobi", -1.3192, 36.9278],
        ["ADD", "Addis Ababa", 8.9779, 38.7993], ["CPT", "Cape Town", -33.9715, 18.6021], ["LOS", "Lagos", 6.5774, 3.3212],
    ]
}

// [IATA code, hub, registration pattern (# digit, ? letter)]
fn carrier_table() {
    [
        ["BA", "LHR", "G-????"], ["AF", "CDG", "F-????"], ["LH", "FRA", "D-A???"], ["KL", "AMS", "PH-???"],
        ["IB", "MAD", "EC-???"], ["VY", "BCN", "EC-???"], ["AZ", "FCO", "EI-???"], ["LX", "ZRH", "HB-???"],
        ["SK", "CPH", "OY-???"], ["EI", "DUB", "EI-???"], ["TK", "IST", "TC-???"], ["MS", "CAI", "SU-???"],
        ["SA", "JNB", "ZS-???"], ["EK", "DXB", "A6-???"], ["QR", "DOH", "A7-???"], ["AI", "DEL", "VT-???"],
        ["SQ", "SIN", "9V-S??"], ["CX", "HKG", "B-H??"], ["CA", "PEK", "B-####"], ["MU", "PVG", "B-####"],
        ["KE", "ICN", "HL####"], ["JL", "NRT", "JA###?"], ["QF", "SYD", "VH-???"], ["B6", "JFK", "N###??"],
        ["UA", "ORD", "N###UA"], ["DL", "ATL", "N###DN"], ["AA", "DFW", "N###AN"], ["AC", "YYZ", "C-????"],
        ["AS", "SEA", "N###AS"], ["AM", "MEX", "XA-???"], ["LA", "GRU", "PR-???"], ["AR", "EZE", "LV-???"],
        ["AV", "BOG", "N###AV"], ["NZ", "AKL", "ZK-???"], ["TG", "BKK", "HS-???"], ["MH", "KUL", "9M-M??"],
        ["CI", "TPE", "B-18###"], ["PR", "MNL", "RP-C####"], ["ET", "ADD", "ET-???"], ["KQ", "NBO", "5Y-???"],
    ]
}

// [ICAO type, seats, cruise kts, cruise ft, longest route nm, shortest route nm]
fn aircraft_table() {
    [
        ["E190", 100, 447.0, 37000.0, 2400.0, 0.0], ["A320", 180, 450.0, 37000.0, 3300.0, 0.0],
        ["B738", 189, 453.0, 37000.0, 3000.0, 0.0], ["A21N", 220, 450.0, 37000.0, 4000.0, 0.0],
        ["B789", 290, 488.0, 41000.0, 7600.0, 1500.0], ["A359", 325, 488.0, 41000.0, 9700.0, 1500.0],
        ["B77W", 396, 490.0, 39000.0, 7300.0, 2000.0], ["A388", 525, 490.0, 39000.0, 8000.0, 3000.0],
    ]
}

// Climb at 2,000 and descend at 1,500 ft/min, averaging 65% of cruise speed
// over both. When climb and descent would cover more than 90% of the route,
// both shrink (and the peak altitude with them) to fit.
fn plan_profile(kts, ft, nm) {
    let nm_per_s = 0.65 * kts / 3600.0;
    let climb_s = ft / 2000.0 * 60.0;
    let descent_s = ft / 1500.0 * 60.0;
    let k = (0.9 * nm / (nm_per_s * (climb_s + descent_s))).min(1.0);
    climb_s *= k;
    descent_s *= k;
    let climb_nm = nm_per_s * climb_s;
    let descent_nm = nm_per_s * descent_s;
    let cruise_s = (nm - climb_nm - descent_nm) / kts * 3600.0;
    #{
        peak_ft: ft * k, climb_s: climb_s, cruise_s: cruise_s, descent_s: descent_s,
        climb_nm: climb_nm, descent_nm: descent_nm, airborne: climb_s + cruise_s + descent_s,
    }
}

let retention = seconds(arg(args, "landed_retention", 600));

let airports = #{};
for a in airport_table() {
    airports[a[0]] = #{ code: a[0], city: a[1], lat: a[2], lon: a[3] };
}
let codes = airports.keys();

// Draw carrier, route and type until the route is 3-16 h airborne and
// passes the long-haul bias.
let carrier = (); let origin = (); let dest = (); let ac = (); let nm = 0.0; let p = ();
loop {
    carrier = pick(carrier_table());
    let hub = airports[carrier[1]];
    let other = airports[pick(codes)];
    if other.code == hub.code { continue; }
    if chance(0.5) { origin = hub; dest = other; } else { origin = other; dest = hub; }
    nm = great_circle(origin.lat, origin.lon, dest.lat, dest.lon) / 1.852;
    let types = [];
    for t in aircraft_table() {
        if t[4] >= nm && t[5] <= nm && (nm >= 2500.0 || t[5] == 0.0) { types.push(t); }
    }
    if types.len() == 0 { continue; }
    ac = pick(types);
    p = plan_profile(ac[2], ac[3], nm);
    let hours = p.airborne / 3600.0;
    if hours >= 3.0 && hours <= 16.0 && rand_float(0.0, 1.0) < hours / 16.0 { break; }
}

// The timeline, in sim unix seconds.
let boarding = minutes(30);
let taxi = minutes(12);
let now0 = now_secs();
let scheduled = now0 + minutes(rand_int(40, 90));
let delay_min = if chance(0.15) { rand_int(5, 45) } else { 0 };
let off_block = scheduled + minutes(delay_min);
let takeoff = off_block + taxi;
let eta = takeoff + p.airborne;
let block_s = eta - off_block;
let kts = ac[2];
let wobble = rand_float(0.0, 6.2832);

let id = pad(sim_id(), 8);
let r = row();
r.flight_no = carrier[0] + rand_int(10, 2999);
r.carrier = carrier[0];
r.aircraft_type = ac[0];
r.registration = pattern(carrier[2]);
r.origin = origin.code;
r.origin_city = origin.city;
r.destination = dest.code;
r.destination_city = dest.city;
r.phase = "Scheduled";
r.progress = 0.0;
r.altitude_ft = 0;
r.ground_speed_kts = 0;
r.heading = bearing(origin.lat, origin.lon, dest.lat, dest.lon).round().to_int() % 360;
r.distance_total_nm = nm.round().to_int();
r.distance_remaining_nm = nm.round().to_int();
r.scheduled_departure = wall_in(scheduled - now0);
r.actual_departure = ();
r.eta = wall_in(eta - now0);
r.flight_time = hours_minutes(block_s);
r.flight_time_min = (block_s / 60.0).round().to_int();
r.delay_minutes = delay_min;
r.gate = pick(["A", "B", "C", "D", "E", "F"]) + rand_int(1, 60);
r.pax = rand_int(ac[1] * 13 / 20, ac[1]);
r.seats = ac[1];
r.lat = round_by(origin.lat, 10000.0);
r.lon = round_by(origin.lon, 10000.0);
r.board_order = scheduled.to_int();
upsert(id, r);

wait_until(off_block - boarding);
patch(id, #{ phase: "Boarding" });
wait_until(off_block);
patch(id, #{ phase: "Taxiing", ground_speed_kts: 15, actual_departure: wall_now() });
wait_until(takeoff);

// Airborne: distance flown, altitude and speed from the planned profile.
while now_secs() < eta {
    let t = now_secs();
    let air = t - takeoff;
    let phase = ""; let flown = 0.0; let alt = 0.0; let gs = 0.0;
    if air < p.climb_s {
        let f = air / p.climb_s;
        phase = if air < p.climb_s.min(minutes(5)) { "Takeoff" } else { "Climbing" };
        flown = f * p.climb_nm; alt = f * p.peak_ft; gs = 160.0 + (kts - 160.0) * f;
    } else if air < p.climb_s + p.cruise_s {
        phase = "Cruising";
        flown = p.climb_nm + (air - p.climb_s) * kts / 3600.0;
        alt = p.peak_ft;
        gs = kts * (1.0 + 0.03 * (t / 600.0 + wobble).sin());
    } else {
        let f = ((air - p.climb_s - p.cruise_s) / p.descent_s).min(1.0);
        phase = if t < eta - p.descent_s.min(minutes(5)) { "Descending" } else { "Landing" };
        flown = nm - p.descent_nm + f * p.descent_nm; alt = (1.0 - f) * p.peak_ft; gs = kts - (kts - 150.0) * f;
    }
    flown = flown.min(nm);
    let at = interpolate(origin.lat, origin.lon, dest.lat, dest.lon, flown / nm);
    patch(id, #{
        phase: phase,
        progress: round_by(flown / nm * 100.0, 10.0),
        altitude_ft: ((alt / 100.0).round() * 100.0).to_int(),
        ground_speed_kts: gs.round().to_int(),
        heading: bearing(at.lat, at.lon, dest.lat, dest.lon).round().to_int() % 360,
        distance_remaining_nm: (nm - flown).max(0.0).round().to_int(),
        lat: round_by(at.lat, 10000.0),
        lon: round_by(at.lon, 10000.0),
    });
    sleep((eta - t).min(30.0));
}

patch(id, #{
    phase: "Landed",
    progress: 100.0,
    altitude_ft: 0,
    ground_speed_kts: 0,
    heading: (bearing(dest.lat, dest.lon, origin.lat, origin.lon) + 180.0).round().to_int() % 360,
    distance_remaining_nm: 0,
    lat: round_by(dest.lat, 10000.0),
    lon: round_by(dest.lon, 10000.0),
});
sleep(retention);
delete(id);
