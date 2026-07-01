//! Watch a live faker table in the terminal.
//!
//! Runs a `fifo` effect: a new row appears every second (newest on top) and
//! expires 8–15s later. Each tick the screen is cleared, the current rows are
//! printed as a styled table, then it sleeps and repeats.
//!
//! ```sh
//! cargo run --example fifo_cli
//! ```
//! Ctrl-C to quit.

use std::time::Duration;

use vantage_cli_util::render_records;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_faker::{FakerColumn, FakerTable, FifoEffect};

fn col(name: &str, ty: &str, is_id: bool) -> FakerColumn {
    FakerColumn {
        name: name.into(),
        ty: ty.into(),
        flags: if is_id { vec!["id".into()] } else { vec![] },
    }
}

#[tokio::main]
async fn main() {
    let columns = vec![
        col("id", "string", true),
        col("first_name", "string", false),
        col("email", "string", false),
        col("city", "string", false),
        col("amount", "decimal", false),
    ];

    let table = FakerTable::build(
        "events",
        columns,
        "id",
        Box::new(FifoEffect {
            interval: Duration::from_secs(1),
            retention_lo: Duration::from_secs(8),
            retention_hi: Duration::from_secs(15),
        }),
    );

    loop {
        // Clear the screen and home the cursor (ANSI), then draw the frame.
        print!("\x1B[2J\x1B[H");
        println!("vantage-faker · fifo demo — +1 row/s, expires in 8–15s · Ctrl-C to quit\n");

        let records = table.vista.list_values().await.expect("list faker rows");
        render_records(&records, Some("id"));

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
