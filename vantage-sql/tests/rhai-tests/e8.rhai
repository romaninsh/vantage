// E8: Wrong type — passing select() into where() expects a condition
let sub = select().from("orders").field("user_id");
select().from("users").where(sub)
