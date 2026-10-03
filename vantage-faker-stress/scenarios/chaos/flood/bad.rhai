// Wait a moment, then insert 10k rows in one burst.
sleep(seconds(2));
for i in 0..10000 {
    insert(#{ note: "flood " + i });
}
