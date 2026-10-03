// A parcel: booked, collected, moved through hubs, delivered, archived.
let hubs = ["LON", "MAN", "BHM", "GLA", "BRS"];
let events = [];
let id = insert(#{ status: "Booked", hub: pick(hubs) });
events.push(insert("event", #{ shipment: id, what: "Booked" }));

sleep(minutes(rand_int(5, 30)));
patch(id, #{ status: "Collected" });
events.push(insert("event", #{ shipment: id, what: "Collected" }));

for leg in 0..rand_int(1, 4) {
    sleep(minutes(rand_int(20, 90)));
    let hub = pick(hubs);
    patch(id, #{ status: "In transit", hub: hub });
    events.push(insert("event", #{ shipment: id, what: "Arrived " + hub }));
}

sleep(minutes(rand_int(10, 60)));
patch(id, #{ status: "Delivered" });
events.push(insert("event", #{ shipment: id, what: "Delivered" }));

sleep(hours(2));
for e in events {
    delete("event", e);
}
delete(id);
