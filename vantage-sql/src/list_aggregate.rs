//! Pieces of the list aggregates (`join_concat`, `join_json`) the SQL
//! dialects share. The rows arrive as a select projecting the value as `v`
//! and each ordering expression as `o0`, `o1`…

/// `o0 ASC, o1 DESC`: the rows' order, by the ordering columns' aliases.
#[cfg(any(feature = "postgres", feature = "mysql"))]
pub(crate) fn order_list(order: &[vantage_table::sorting::SortDirection]) -> String {
    use vantage_table::sorting::SortDirection;
    order
        .iter()
        .enumerate()
        .map(|(at, direction)| match direction {
            SortDirection::Ascending => format!("o{at} ASC"),
            SortDirection::Descending => format!("o{at} DESC"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// `text` as a quoted MySQL string literal. `GROUP_CONCAT`'s `SEPARATOR`
/// takes a literal, not a parameter; `{}` would read as a placeholder of the
/// expression template, so it is refused.
#[cfg(feature = "mysql")]
pub(crate) fn mysql_literal(text: &str) -> vantage_core::Result<String> {
    if text.contains("{}") {
        return Err(vantage_core::error!(
            "a join_concat separator can't contain `{}` on MySQL",
            separator = text
        ));
    }
    Ok(format!(
        "'{}'",
        text.replace('\\', "\\\\").replace('\'', "''")
    ))
}
