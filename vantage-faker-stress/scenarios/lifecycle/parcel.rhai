// A parcel: booked, collected, moved through hubs, delivered, archived.
let hubs = ["LON", "MAN", "BHM", "GLA", "BRS"];
let events = [];
let id = table().insert(#{ status: "Booked", hub: pick(hubs) });
events.push(table("event").insert(#{ shipment: id, what: "Booked" }));

sleep(minutes(rand_int(5, 30)));
table().patch(id, #{ status: "Collected" });
events.push(table("event").insert(#{ shipment: id, what: "Collected" }));

for leg in 0..rand_int(1, 4) {
    sleep(minutes(rand_int(20, 90)));
    let hub = pick(hubs);
    table().patch(id, #{ status: "In transit", hub: hub });
    events.push(table("event").insert(#{ shipment: id, what: "Arrived " + hub }));
}

sleep(minutes(rand_int(10, 60)));
table().patch(id, #{ status: "Delivered" });
events.push(table("event").insert(#{ shipment: id, what: "Delivered" }));

sleep(hours(2));
for e in events {
    table("event").delete(e);
}
table().delete(id);
