// E8: Wrong type — passing sql::select() into where() expects a condition
let sub = sql::select().from("orders").field("user_id");
sql::select().from("users").where(sub)
