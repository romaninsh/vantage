// One aircraft: its own row, a position report every 30 sim seconds.
let id = table().insert(#{ lat: rand_float(-60.0, 60.0), lon: rand_float(-180.0, 180.0) });
loop {
    sleep(seconds(30));
    let row = table().get(id);
    table().patch(id, #{ lat: row.lat + rand_float(-0.1, 0.1), lon: row.lon + rand_float(-0.1, 0.1) });
}
