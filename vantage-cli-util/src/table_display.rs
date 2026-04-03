use comfy_table::{Attribute, Cell, Color, Table as ComfyTable};
use indexmap::IndexMap;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_table::prelude::ColumnLike;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record, TerminalRender};

/// Map a color hint string to a comfy-table Color.
fn hint_to_color(hint: &str) -> Option<Color> {
    match hint {
        "green" => Some(Color::Green),
        "red" => Some(Color::Red),
        "blue" => Some(Color::Blue),
        "yellow" => Some(Color::Yellow),
        "cyan" => Some(Color::Cyan),
        "magenta" => Some(Color::Magenta),
        _ => None,
    }
}

/// Create a styled Cell from a TerminalRender value.
fn render_cell<V: TerminalRender>(value: &V) -> Cell {
    let text = value.render();
    let mut cell = Cell::new(&text);

    if let Some(hint) = value.color_hint() {
        if hint == "dim" {
            cell = cell.add_attribute(Attribute::Dim);
        } else if let Some(color) = hint_to_color(hint) {
            cell = cell.fg(color);
        }
    }

    cell
}

/// Fetch records from a table and print them as a formatted ASCII table.
///
/// Reads the id field from the table's column flags to avoid duplicating
/// the id column in the output.
pub async fn print_table<T, E>(table: &Table<T, E>) -> vantage_core::Result<()>
where
    T: TableSource,
    T::Value: TerminalRender,
    T::Id: std::fmt::Display,
    E: Entity<T::Value>,
{
    let id_field = table.id_field().map(|col| col.name().to_string());
    let records = table.list_values().await?;
    render_records(&records, id_field.as_deref());
    Ok(())
}

/// Render records as a formatted ASCII table.
///
/// `id_field` names the column used as the record key. That column is
/// skipped in the data columns to avoid duplication.
pub fn render_records<Id: std::fmt::Display, V: TerminalRender>(
    records: &IndexMap<Id, Record<V>>,
    id_field: Option<&str>,
) {
    if records.is_empty() {
        println!("No records found.");
        return;
    }

    let first_record = records.values().next().unwrap();
    let columns: Vec<&String> = first_record
        .keys()
        .filter(|k| k.as_str() != "id" && Some(k.as_str()) != id_field)
        .collect();

    let mut table = ComfyTable::new();

    let mut header = vec![Cell::new("id").add_attribute(Attribute::Bold)];
    header.extend(
        columns
            .iter()
            .map(|c| Cell::new(c).add_attribute(Attribute::Bold)),
    );
    table.set_header(header);

    for (id, record) in records {
        let mut row = vec![Cell::new(id)];
        for col in &columns {
            if let Some(value) = record.get(col.as_str()) {
                row.push(render_cell(value));
            } else {
                row.push(Cell::new(""));
            }
        }
        table.add_row(row);
    }

    println!("{table}");
    println!("Found {} records", records.len());
}
