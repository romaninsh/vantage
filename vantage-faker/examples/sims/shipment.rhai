// Shipment — a parcel from booking to delivery, for a logistics demo.
//
// Writes two tables: its own row in `shipment` (the def's table) and one
// `tracking_event` row per step. Status runs Booked -> Picked up ->
// In transit -> Out for delivery -> Delivered, with the odd Exception that
// holds the parcel and marks it late. Two days after delivery the shipment
// and its events are deleted.
//
// Suggested def:
//   table: shipment, clock: 60, warm: 3d
//   spawn: burst 20, rate 0.1 per sim minute, max 400
//
// shipment columns: id, tracking_no, customer, origin, destination, status,
//   eta, lat, lon, late, booked_at
// tracking_event columns: id, shipment_id, status, note, lat, lon, at

let hubs = [
    #{ city: "Rotterdam", lat: 51.9244, lon: 4.4777 },
    #{ city: "Hamburg", lat: 53.5511, lon: 9.9937 },
    #{ city: "Antwerp", lat: 51.2194, lon: 4.4025 },
    #{ city: "Paris", lat: 48.8566, lon: 2.3522 },
    #{ city: "Lyon", lat: 45.7640, lon: 4.8357 },
    #{ city: "Madrid", lat: 40.4168, lon: -3.7038 },
    #{ city: "Milan", lat: 45.4642, lon: 9.1900 },
    #{ city: "Munich", lat: 48.1351, lon: 11.5820 },
    #{ city: "Warsaw", lat: 52.2297, lon: 21.0122 },
    #{ city: "Prague", lat: 50.0755, lon: 14.4378 },
    #{ city: "Vienna", lat: 48.2082, lon: 16.3738 },
    #{ city: "Copenhagen", lat: 55.6761, lon: 12.5683 },
];

let origin = pick(hubs);
let dest = pick(hubs);
while dest.city == origin.city {
    dest = pick(hubs);
}

// The plan, in sim seconds: trucking at about 55 km/h door to door.
let km = great_circle(origin.lat, origin.lon, dest.lat, dest.lon);
let transit = km / 55.0 * 3600.0;
let pickup_wait = hours(rand_float(2.0, 10.0));
let delivery_window = hours(6);
let due = now_secs() + pickup_wait + transit + delivery_window;
let late = false;

let id = insert(#{
    tracking_no: pattern("SH-########"),
    customer: fake("company"),
    origin: origin.city,
    destination: dest.city,
    status: "Booked",
    eta: wall_in(due - now_secs()),
    lat: origin.lat,
    lon: origin.lon,
    late: false,
    booked_at: wall_now(),
});

let track = |status, lat, lon, note| {
    insert("tracking_event", #{
        shipment_id: id,
        status: status,
        note: note,
        lat: (lat * 10000.0).round() / 10000.0,
        lon: (lon * 10000.0).round() / 10000.0,
        at: wall_now(),
    });
};
track.call("Booked", origin.lat, origin.lon, "Label created");

sleep(pickup_wait);
patch(id, #{ status: "Picked up" });
track.call("Picked up", origin.lat, origin.lon, "Collected from sender");

let legs = rand_int(3, 6);
let leg_time = transit / legs.to_float();
for leg in 1..=legs {
    sleep(leg_time);
    let p = interpolate(origin.lat, origin.lon, dest.lat, dest.lon, leg.to_float() / legs.to_float());
    if chance(0.08) {
        let hold = hours(rand_float(4.0, 24.0));
        let reason = pick(["Address query", "Customs hold", "Damaged label", "Weather delay"]);
        late = true;
        due += hold;
        patch(id, #{ status: "Exception", late: true, lat: p.lat, lon: p.lon, eta: wall_in(due - now_secs()) });
        track.call("Exception", p.lat, p.lon, reason);
        sleep(hold);
    }
    patch(id, #{ status: "In transit", lat: p.lat, lon: p.lon });
    track.call("In transit", p.lat, p.lon, if leg == legs { "Arrived at delivery depot" } else { "Departed hub" });
}

patch(id, #{ status: "Out for delivery", lat: dest.lat, lon: dest.lon });
track.call("Out for delivery", dest.lat, dest.lon, "With courier");
sleep(hours(rand_float(1.0, 8.0)));
late = late || now_secs() > due;
patch(id, #{ status: "Delivered", late: late, eta: wall_now() });
track.call("Delivered", dest.lat, dest.lon, "Signed for");

sleep(days(2));
for e in ids("tracking_event") {
    let ev = get("tracking_event", e);
    if type_of(ev) == "map" && ev.shipment_id == id {
        delete("tracking_event", e);
    }
}
delete(id);
