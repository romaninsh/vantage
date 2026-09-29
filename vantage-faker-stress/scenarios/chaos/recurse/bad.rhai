// Recurse until something stops it.
fn dive(n) {
    dive(n + 1)
}
sleep(seconds(1));
dive(0);
