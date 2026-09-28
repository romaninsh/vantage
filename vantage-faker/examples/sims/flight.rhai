// Flight — one aircraft from the departure board to the gate at the far end.
//
// A linear port of the `flights` effect: each sim is one flight. It inserts
// its row, sleeps through boarding and taxi, patches its position every 30
// sim seconds while airborne, shows `Landed` for ten minutes, then deletes
// the row.
//
// Suggested def (about 200 in the air, 1-2 landing at any moment):
//   table: flights, clock: 10, warm: 12h
//   spawn: burst 0, rate 0.3 per sim minute, max 260
//
// Timestamps (`scheduled_departure`, `actual_departure`, `eta`) are
// wall-clock, so a UI can say "lands in 4 min"; `flight_time` is sim time.

let airports = [
    #{ code: "LHR", city: "London", lat: 51.4700, lon: -0.4543 },
    #{ code: "CDG", city: "Paris", lat: 49.0097, lon: 2.5479 },
    #{ code: "FRA", city: "Frankfurt", lat: 50.0379, lon: 8.5622 },
    #{ code: "IST", city: "Istanbul", lat: 41.2753, lon: 28.7519 },
    #{ code: "DXB", city: "Dubai", lat: 25.2532, lon: 55.3657 },
    #{ code: "DEL", city: "Delhi", lat: 28.5562, lon: 77.1000 },
    #{ code: "SIN", city: "Singapore", lat: 1.3644, lon: 103.9915 },
    #{ code: "HKG", city: "Hong Kong", lat: 22.3080, lon: 113.9185 },
    #{ code: "NRT", city: "Tokyo", lat: 35.7720, lon: 140.3929 },
    #{ code: "SYD", city: "Sydney", lat: -33.9399, lon: 151.1753 },
    #{ code: "JNB", city: "Johannesburg", lat: -26.1367, lon: 28.2411 },
    #{ code: "GRU", city: "Sao Paulo", lat: -23.4356, lon: -46.4731 },
    #{ code: "JFK", city: "New York", lat: 40.6413, lon: -73.7781 },
    #{ code: "ORD", city: "Chicago", lat: 41.9742, lon: -87.9073 },
    #{ code: "LAX", city: "Los Angeles", lat: 33.9416, lon: -118.4085 },
    #{ code: "YVR", city: "Vancouver", lat: 49.1967, lon: -123.1815 },
];
let carriers = ["BA", "AF", "LH", "TK", "EK", "AI", "SQ", "CX", "JL", "QF", "SA", "LA", "UA", "AA"];

// A long-haul pair: at least 2,500 km apart.
let origin = pick(airports);
let dest = pick(airports);
let km = great_circle(origin.lat, origin.lon, dest.lat, dest.lon);
while km < 2500.0 {
    dest = pick(airports);
    km = great_circle(origin.lat, origin.lon, dest.lat, dest.lon);
}

// The plan, in sim seconds.
let airborne = km / 880.0 * 3600.0 + minutes(25);
let taxi = minutes(12);
let boarding = minutes(30);
let delay = if chance(0.15) { minutes(rand_int(5, 45)) } else { 0.0 };
let depart_in = minutes(rand_int(40, 90));
let block_min = ((taxi + airborne) / 60.0).round().to_int();

let id = insert(#{
    flight_no: pick(carriers) + rand_int(10, 2999),
    origin: origin.code,
    origin_city: origin.city,
    destination: dest.code,
    destination_city: dest.city,
    phase: "Scheduled",
    progress: 0.0,
    altitude_ft: 0,
    ground_speed_kts: 0,
    heading: bearing(origin.lat, origin.lon, dest.lat, dest.lon).round().to_int(),
    lat: origin.lat,
    lon: origin.lon,
    scheduled_departure: wall_in(depart_in),
    actual_departure: (),
    eta: wall_in(depart_in + delay + taxi + airborne),
    flight_time: `${block_min / 60}h ${block_min % 60}m`,
    flight_time_min: block_min,
    delay_minutes: (delay / 60.0).to_int(),
});

sleep(depart_in + delay - boarding);
patch(id, #{ phase: "Boarding" });
sleep(boarding);
patch(id, #{ phase: "Taxiing", ground_speed_kts: 15, actual_departure: wall_now() });
sleep(taxi);

let cruise_ft = 38000.0;
let t = 0.0;
while t < airborne {
    let to_go = airborne - t;
    let frac = t / airborne;
    let p = interpolate(origin.lat, origin.lon, dest.lat, dest.lon, frac);
    let phase = if t < minutes(5) { "Takeoff" }
        else if t < minutes(20) { "Climbing" }
        else if to_go <= minutes(5) { "Landing" }
        else if to_go <= minutes(25) { "Descending" }
        else { "Cruising" };
    let alt = if t < minutes(20) { cruise_ft * t / minutes(20) }
        else if to_go < minutes(25) { cruise_ft * to_go / minutes(25) }
        else { cruise_ft };
    patch(id, #{
        phase: phase,
        progress: (frac * 1000.0).round() / 10.0,
        altitude_ft: ((alt / 100.0).round() * 100.0).to_int(),
        ground_speed_kts: if phase == "Cruising" { rand_int(465, 495) } else { rand_int(160, 300) },
        heading: bearing(p.lat, p.lon, dest.lat, dest.lon).round().to_int(),
        lat: (p.lat * 10000.0).round() / 10000.0,
        lon: (p.lon * 10000.0).round() / 10000.0,
    });
    let step = if to_go < seconds(30) { to_go } else { seconds(30) };
    sleep(step);
    t += step;
}

patch(id, #{
    phase: "Landed",
    progress: 100.0,
    altitude_ft: 0,
    ground_speed_kts: 0,
    lat: dest.lat,
    lon: dest.lon,
});
sleep(minutes(10));
delete(id);
