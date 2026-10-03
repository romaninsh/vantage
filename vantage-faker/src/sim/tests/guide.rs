//! The order sim shown in the README and in the Vantage book's Rhai guide.

use super::*;

const ORDER: &str = r#"
if table().where("status", "Placed").count() >= 40 {
    done();
}

let id = table().insert(#{ customer: fake("name"), total: rand_float(5.0, 80.0), status: "Placed" });
sleep(minutes(rand_int(5, 20)));

table().patch(id, #{ status: "Shipped" });
table("order_event").insert(#{ order: id, note: "shipped" });
sleep(hours(2));

table().delete(id);
"#;

#[test]
fn order_sim_from_the_guide() {
    let store = store_with(&["order", "order_event"]);
    let engine = SimEngine::builder()
        .store(&store)
        .sim(SimDef::new("order", "order", ORDER))
        .manual_clock(start())
        .seed(7)
        .start()
        .unwrap();
    let orders = store.table("order");

    run_for(&engine, 1, 1);
    let placed = rows(&orders);
    assert_eq!(placed.len(), 1);
    assert_eq!(text(&placed[0], "status"), "Placed");

    run_for(&engine, 21 * 60, 30);
    assert_eq!(text(&rows(&orders)[0], "status"), "Shipped");
    assert_eq!(store.table("order_event").len(), 1);

    run_for(&engine, 2 * 3600, 60);
    assert!(rows(&orders).is_empty());
}
