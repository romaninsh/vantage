// Create a row, delete it, then keep writing to it.
loop {
    let id = insert(#{ note: "doomed" });
    delete(id);
    sleep(seconds(1));
    patch(id, #{ note: "late" });
    set(id, "note", "later");
    delete(id);
}
