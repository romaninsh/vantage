// Every aircraft moves in one pass, every 30 sim seconds.
loop {
    for id in table().ids() {
        let row = table().get(id);
        table().patch(id, #{ lat: row.lat + rand_float(-0.1, 0.1), lon: row.lon + rand_float(-0.1, 0.1) });
    }
    sleep(seconds(30));
}
