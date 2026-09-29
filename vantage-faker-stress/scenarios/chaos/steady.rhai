// A well-behaved neighbour: one row, patched every second.
let id = insert(#{ note: "steady" });
loop {
    sleep(seconds(1));
    patch(id, #{ note: "tick " + rand_int(0, 1000) });
}
