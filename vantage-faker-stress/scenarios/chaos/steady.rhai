// A well-behaved neighbour: one row, patched every second.
let id = table().insert(#{ note: "steady" });
loop {
    sleep(seconds(1));
    table().patch(id, #{ note: "tick " + rand_int(0, 1000) });
}
