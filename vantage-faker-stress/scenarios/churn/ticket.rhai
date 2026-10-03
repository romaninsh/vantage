// One ticket: opened, worked on, closed, then gone.
let id = table().insert(#{ status: "Open", amount: rand_int(1, 500) });
sleep(seconds(rand_int(20, 120)));
table().patch(id, #{ status: "Pending" });
sleep(seconds(rand_int(20, 120)));
table().patch(id, #{ status: "Closed" });
sleep(seconds(30));
table().delete(id);
