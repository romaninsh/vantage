use comfy_table::{Attribute, Cell, Color, Table};
use indexmap::IndexMap;
use vantage_types::{Record, TerminalRender};

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

/// Print records as a formatted ASCII table.
///
/// Works with any value type that implements `TerminalRender`.
/// Columns are auto-detected from the first record's keys.
pub fn print_table<V: TerminalRender>(records: &IndexMap<String, Record<V>>) {
    if records.is_empty() {
        println!("No records found.");
        return;
    }

    let first_record = records.values().next().unwrap();
    let columns: Vec<&String> = first_record
        .keys()
        .filter(|k| k.as_str() != "id")
        .collect();

    let mut table = Table::new();

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

/// Print records showing only specific columns.
pub fn print_table_columns<V: TerminalRender>(
    records: &IndexMap<String, Record<V>>,
    columns: &[&str],
) {
    if records.is_empty() {
        println!("No records found.");
        return;
    }

    let mut table = Table::new();

    let mut header = vec![Cell::new("id").add_attribute(Attribute::Bold)];
    header.extend(
        columns
            .iter()
            .map(|c| Cell::new(c).add_attribute(Attribute::Bold)),
    );
    table.set_header(header);

    for (id, record) in records {
        let mut row = vec![Cell::new(id)];
        for col in columns {
            if let Some(value) = record.get(*col) {
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
