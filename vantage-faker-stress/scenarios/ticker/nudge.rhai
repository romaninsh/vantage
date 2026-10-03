// Move every balance by up to 5 either way, ten times a second.
loop {
    for id in table().ids() {
        let row = table().get(id);
        table().patch(id, #{ balance: row.balance + rand_int(-5, 5) });
    }
    sleep(seconds(0.1));
}
