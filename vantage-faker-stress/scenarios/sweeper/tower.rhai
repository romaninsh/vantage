// Every aircraft moves in one pass, every 30 sim seconds.
loop {
    for id in ids() {
        let row = get(id);
        patch(id, #{ lat: row.lat + rand_float(-0.1, 0.1), lon: row.lon + rand_float(-0.1, 0.1) });
    }
    sleep(seconds(30));
}
