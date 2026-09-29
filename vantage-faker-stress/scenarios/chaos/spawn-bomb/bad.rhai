// Start two more of myself, then idle.
spawn_sim("bomb", #{});
spawn_sim("bomb", #{});
loop {
    sleep(minutes(10));
}
