// One ticket: opened, worked on, closed, then gone.
let id = insert(#{ status: "Open", amount: rand_int(1, 500) });
sleep(seconds(rand_int(20, 120)));
patch(id, #{ status: "Pending" });
sleep(seconds(rand_int(20, 120)));
patch(id, #{ status: "Closed" });
sleep(seconds(30));
delete(id);
