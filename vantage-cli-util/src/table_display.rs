use comfy_table::{
    Attribute, Cell, CellAlignment, ContentArrangement, Table as ComfyTable, TableComponent,
    presets,
};
use indexmap::IndexMap;
use owo_colors::OwoColorize;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_table::prelude::ColumnLike;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record, RichText, Span, Style, TerminalRender};

/// Convert a [`RichText`] to an ANSI-styled string.
///
/// `owo-colors` honors `NO_COLOR` and auto-detects whether stdout is
/// a TTY, so this is safe to embed in cells regardless of context.
fn rich_to_ansi(rich: &RichText) -> String {
    let mut out = String::with_capacity(rich.spans.iter().map(|s| s.text.len()).sum::<usize>() + 8);
    for span in &rich.spans {
        out.push_str(&span_to_ansi(span));
    }
    out
}

fn span_to_ansi(span: &Span) -> String {
    let t = span.text.as_str();
    match span.style {
        Style::Default => t.to_string(),
        Style::Dim => t.dimmed().to_string(),
        Style::Muted => t.bright_black().to_string(),
        Style::Strong => t.bold().to_string(),
        Style::Success => t.green().to_string(),
        Style::Error => t.red().to_string(),
        Style::Warning => t.yellow().to_string(),
        Style::Info => t.cyan().to_string(),
    }
}

/// Pick alignment from the column's declared Rust type name.
fn alignment_for(type_name: &str) -> CellAlignment {
    let last = type_name.rsplit("::").next().unwrap_or(type_name);
    if matches!(
        last,
        "i8" | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
    ) {
        CellAlignment::Right
    } else {
        CellAlignment::Left
    }
}

fn header_cell(name: &str) -> Cell {
    Cell::new(name.to_uppercase().cyan().bold().to_string())
}

/// Fetch records from a table and print them as a styled table.
pub async fn print_table<T, E>(table: &Table<T, E>) -> vantage_core::Result<()>
where
    T: TableSource,
    T::Value: TerminalRender,
    T::Id: std::fmt::Display,
    E: Entity<T::Value>,
{
    let id_field = table.id_field().map(|c| c.name().to_string());

    let column_types: IndexMap<String, &'static str> = table
        .columns()
        .iter()
        .map(|(name, col)| (name.clone(), col.get_type()))
        .collect();

    let records = table.list_values().await?;
    render_records_typed(&records, id_field.as_deref(), &column_types);
    Ok(())
}

/// Render records as a styled table without per-column type metadata.
///
/// Convenience wrapper around [`render_records_typed`] for ad-hoc maps
/// where column types aren't available.
pub fn render_records<Id, V>(records: &IndexMap<Id, Record<V>>, id_field: Option<&str>)
where
    Id: std::fmt::Display,
    V: TerminalRender,
{
    render_records_typed(records, id_field, &IndexMap::new());
}

/// Render records with an explicit column list — no auto-prepended
/// id column. `column_types` is consulted for per-column alignment but
/// doesn't drive which columns appear; `columns` does.
///
/// Used by the model-driven CLI when the caller passes `=col1,col2`:
/// the user spelled out exactly what they want to see, and an extra
/// id column would just be noise.
pub fn render_records_columns<Id, V>(
    records: &IndexMap<Id, Record<V>>,
    columns: &[String],
    column_types: &IndexMap<String, &'static str>,
) where
    Id: std::fmt::Display,
    V: TerminalRender,
{
    if records.is_empty() {
        println!("{}", "No records.".dimmed());
        return;
    }

    let mut table = ComfyTable::new();
    table
        .load_preset(presets::UTF8_HORIZONTAL_ONLY)
        .remove_style(TableComponent::HorizontalLines)
        .remove_style(TableComponent::MiddleIntersections)
        .remove_style(TableComponent::LeftBorderIntersections)
        .remove_style(TableComponent::RightBorderIntersections)
        .set_content_arrangement(ContentArrangement::Disabled);

    let header: Vec<Cell> = columns.iter().map(|c| header_cell(c)).collect();
    table.set_header(header);

    for (idx, name) in columns.iter().enumerate() {
        if let Some(type_name) = column_types.get(name) {
            let align = alignment_for(type_name);
            if let Some(col) = table.column_mut(idx) {
                col.set_cell_alignment(align);
            }
        }
    }

    for (_id, record) in records {
        let row: Vec<Cell> = columns
            .iter()
            .map(|col| match record.get(col.as_str()) {
                Some(value) => Cell::new(rich_to_ansi(&value.render())),
                None => Cell::new("—".bright_black().to_string()),
            })
            .collect();
        table.add_row(row);
    }

    println!("{table}");
    let n = records.len();
    let label = if n == 1 { "record" } else { "records" };
    println!("{}", format!("{n} {label}").dimmed());
}

/// Render records as a styled table.
///
/// `id_field` names the column used as the record key — printed as the
/// leftmost column and skipped from the data section.
///
/// `column_types` maps column name → declared Rust type name (from
/// `column.get_type()`). Drives per-column alignment and column order.
pub fn render_records_typed<Id, V>(
    records: &IndexMap<Id, Record<V>>,
    id_field: Option<&str>,
    column_types: &IndexMap<String, &'static str>,
) where
    Id: std::fmt::Display,
    V: TerminalRender,
{
    if records.is_empty() {
        println!("{}", "No records.".dimmed());
        return;
    }

    let columns: Vec<String> = if !column_types.is_empty() {
        column_types
            .keys()
            .filter(|k| Some(k.as_str()) != id_field)
            .cloned()
            .collect()
    } else {
        records
            .values()
            .next()
            .unwrap()
            .keys()
            .filter(|k| k.as_str() != "id" && Some(k.as_str()) != id_field)
            .cloned()
            .collect()
    };

    let mut table = ComfyTable::new();
    table
        .load_preset(presets::UTF8_HORIZONTAL_ONLY)
        // Drop inter-row separators — keep only top, header, and bottom rules.
        .remove_style(TableComponent::HorizontalLines)
        .remove_style(TableComponent::MiddleIntersections)
        .remove_style(TableComponent::LeftBorderIntersections)
        .remove_style(TableComponent::RightBorderIntersections)
        // Size columns to their content. `Dynamic` stretches to fill
        // terminal width, which looks bad when piped or when the
        // terminal can't be detected.
        .set_content_arrangement(ContentArrangement::Disabled);

    let mut header = vec![header_cell("id")];
    header.extend(columns.iter().map(|c| header_cell(c)));
    table.set_header(header);

    for (idx, name) in columns.iter().enumerate() {
        if let Some(type_name) = column_types.get(name) {
            let align = alignment_for(type_name);
            if let Some(col) = table.column_mut(idx + 1) {
                col.set_cell_alignment(align);
            }
        }
    }

    for (id, record) in records {
        let id_cell = Cell::new(id.to_string()).add_attribute(Attribute::Bold);
        let mut row = vec![id_cell];
        for col in &columns {
            let cell = match record.get(col.as_str()) {
                Some(value) => Cell::new(rich_to_ansi(&value.render())),
                None => Cell::new("—".bright_black().to_string()),
            };
            row.push(cell);
        }
        table.add_row(row);
    }

    println!("{table}");

    let n = records.len();
    let label = if n == 1 { "record" } else { "records" };
    println!("{}", format!("{n} {label}").dimmed());
}
