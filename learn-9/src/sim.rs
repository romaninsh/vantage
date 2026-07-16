use std::time::Duration;

use vantage_sql::prelude::*;

use crate::db::Db;

/// The bar's till — identical to the SQLite-only version. Every 800ms it sells
/// one unit of a random in-stock product, clears anything that sells out, and
/// every tenth tick takes a delivery that restocks the shelf. The SQL is plain
/// enough (`$1` placeholders, `ON CONFLICT`, `RANDOM()`) that both SQLite and
/// PostgreSQL run it unchanged.
pub fn spawn(db: Db) {
    tokio::spawn(async move {
        let mut tick: u64 = 0;
        loop {
            tokio::time::sleep(Duration::from_millis(800)).await;
            tick += 1;
            if let Err(e) = step(&db, tick).await {
                e.report();
            }
        }
    });
}

async fn step(db: &Db, tick: u64) -> VantageResult<()> {
    // Sell one unit of a random product that still has stock.
    sqlx::query(
        "UPDATE product SET stock = stock - 1
         WHERE id = (SELECT id FROM product WHERE stock > 0 ORDER BY RANDOM() LIMIT 1)",
    )
    .execute(db.pool())
    .await
    .context("sell")?;

    // Anything that hit zero leaves the shelf.
    sqlx::query("DELETE FROM product WHERE stock <= 0")
        .execute(db.pool())
        .await
        .context("sell-out")?;

    // Every tenth tick a delivery restocks the bar.
    if tick.is_multiple_of(10) {
        let id = format!("d{tick}");
        sqlx::query(
            "INSERT INTO product (id, name, price, stock) VALUES ($1, $2, $3, $4)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(&id)
        .bind(format!("Delivery #{tick}"))
        .bind(300_i64)
        .bind(15_i64)
        .execute(db.pool())
        .await
        .context("delivery")?;
    }
    Ok(())
}
