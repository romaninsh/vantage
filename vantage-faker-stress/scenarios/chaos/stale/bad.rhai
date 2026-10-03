// Create a row, delete it, then keep writing to it.
loop {
    let id = table().insert(#{ note: "doomed" });
    table().delete(id);
    sleep(seconds(1));
    table().patch(id, #{ note: "late" });
    table().patch(id, #{ note: "later" });
    table().delete(id);
}
