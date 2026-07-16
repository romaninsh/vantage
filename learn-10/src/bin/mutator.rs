//! The bar's till — a *separate process* from the server.
//!
//! Run the server (`cargo run -p learn-10`) and this (`cargo run -p learn-10
//! --bin mutator`) side by side. The server never writes to `product`; every
//! change on screen comes from this binary, through the database, via
//! `LISTEN/NOTIFY`. That separation is the point: it proves the UI updates
//! because the *database* changed, not because one process poked another.
//!
//! It makes exactly one change at a time — a single sale (or a delivery when
//! the shelf runs low) — with a short *random* pause between them, so updates
//! arrive irregularly and never look like a poll on a fixed interval. Stock
//! only ever goes down; a drink that sells its last unit leaves the shelf.
//!
//! Writes go through Vantage's **active-entity** API: `list_entities()` hands
//! back drinks that carry their own id and datasource, so selling one is just
//! `drink.stock -= 1; drink.save()` and clearing it is `drink.delete()` — no
//! table or id threaded through the call.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use learn_10::db;
use learn_10::product::Product;
use rand::Rng;
use rand::seq::SliceRandom;
use vantage_dataset::prelude::ActiveEntitySet;
use vantage_sql::postgres::PostgresDB;
use vantage_sql::prelude::*;

/// The bar menu deliveries are drawn from: (name, price in cents).
const MENU: &[(&str, i64)] = &[
    ("Negroni", 1200),
    ("Old Fashioned", 1300),
    ("Margarita", 1100),
    ("Mojito", 1000),
    ("Aperol Spritz", 950),
    ("Dry Martini", 1400),
    ("Manhattan", 1350),
    ("Daiquiri", 1050),
    ("Whiskey Sour", 1150),
    ("Cosmopolitan", 1250),
    ("Espresso Martini", 1300),
    ("Paloma", 1000),
    ("Boulevardier", 1400),
    ("Sazerac", 1450),
    ("Gimlet", 1100),
];

/// A fresh, full bottle arrives once this many units have been poured, so
/// delivery simply tracks consumption and the shelf balances itself. A short
/// bootstrap keeps a few drinks on hand so there's always something to pour.
const UNITS_PER_DELIVERY: i64 = 12;
const BOOTSTRAP_MIN: usize = 3;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        e.report();
    }
}

async fn run() -> VantageResult<()> {
    let db = db::connect().await?;
    db::setup(&db).await?;
    println!("mutator running — one change at a time (Ctrl-C to stop)");

    let mut sold_since_delivery: i64 = 0;
    loop {
        // Event-paced, not a fixed beat: a short *random* pause, so the stream
        // is clearly event-driven rather than a timer.
        let pause = rand::thread_rng().gen_range(200..1000);
        tokio::time::sleep(Duration::from_millis(pause)).await;

        let table = Product::table(db.clone());
        // The whole shelf, as active entities — each knows how to save or
        // delete itself.
        let mut shelf = table.list_entities().await?;

        // Bootstrap an empty bar, or send a fresh bottle once a bottle's worth
        // has been poured. Otherwise pour a single unit.
        if shelf.len() < BOOTSTRAP_MIN || sold_since_delivery >= UNITS_PER_DELIVERY {
            if sold_since_delivery >= UNITS_PER_DELIVERY {
                sold_since_delivery -= UNITS_PER_DELIVERY;
            }
            deliver(&table).await?;
        } else {
            // Pour one unit of a random drink; the last unit takes it off the
            // shelf. No table or id at the call site — the entity carries both.
            let drink = shelf.choose_mut(&mut rand::thread_rng()).unwrap();
            if drink.stock <= 1 {
                println!("🍸 {} sold out — off the shelf", drink.name);
                drink.delete().await?;
            } else {
                drink.stock -= 1;
                println!("🍸 sold {}: {} → {}", drink.name, drink.stock + 1, drink.stock);
                drink.save().await?;
            }
            sold_since_delivery += 1;
        }
    }
}

/// A delivery: a named drink arrives with a full crate of stock.
async fn deliver(table: &Table<PostgresDB, Product>) -> VantageResult<()> {
    let (name, price, stock, id) = {
        let mut r = rand::thread_rng();
        let (name, price) = *MENU.choose(&mut r).unwrap();
        let id = format!("d{}", r.gen_range(0..1_000_000_000u32));
        (name, price, r.gen_range(10..15), id)
    };
    table
        .new_entity(
            id,
            Product {
                name: name.to_string(),
                price,
                stock,
                created: now_millis(),
            },
        )
        .save()
        .await?;
    println!("📦 delivery: {name} × {stock}");
    Ok(())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
