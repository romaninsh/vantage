use std::sync::Arc;

use anyhow::Result;
use bakery_model::postgres;
use sqlformat::{FormatOptions, QueryParams};
use vantage::{
    prelude::*,
    sql::query::{self, QueryConditions, SqlQuery},
};

async fn create_bootstrap_db() -> Result<()> {
    bakery_model::connect_postgres().await?;
    let vantage_client = bakery_model::postgres();
    let client = vantage_client.client();
    let schema = tokio::fs::read_to_string("schema-calendar-pg.sql").await?;
    sqlx::raw_sql(&schema).execute(client).await?;

    Ok(())
}

pub fn format_query(q: &Query) -> String {
    let qs = q.render_chunk().split();

    let formatted_sql = sqlformat::format(
        &qs.0.replace("{}", "?"),
        &QueryParams::Indexed(qs.1.iter().map(|x| x.to_string()).collect::<Vec<String>>()),
        &FormatOptions::default(),
    );

    formatted_sql

    // let ps = SyntaxSet::load_defaults_newlines();
    // let ts = ThemeSet::load_defaults();

    // // Choose a theme
    // let theme = &ts.themes["base16-ocean.dark"];

    // // Get the syntax definition for SQL
    // let syntax = ps.find_syntax_by_extension("sql").unwrap();

    // // Create a highlighter
    // let mut h = HighlightLines::new(syntax, theme);

    // // Apply highlighting
    // let mut highlighted_sql = String::new();
    // for line in LinesWithEndings::from(&formatted_sql) {
    //     let ranges: Vec<(Style, &str)> = h.highlight_line(line, &ps).unwrap();
    //     let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
    //     highlighted_sql.push_str(&escaped);
    // }

    // highlighted_sql
}

fn generate_series(from: Expression, to: Expression, inter: &str) -> Expression {
    expr_arc!(
        "SELECT generate_series({}, {}, {})",
        from,
        to,
        inter.to_string()
    )
    .render_chunk()
}

fn today() -> Expression {
    expr!("date_trunc('day', now())")
}

fn plus_interval(e: Expression, interval: &str) -> Expression {
    expr_arc!("{} + INTERVAL {}", e, interval.to_string()).render_chunk()
}

fn find_available_slots(field_name: &str, events: impl TableWithQueries) -> Query {
    let generate_series = generate_series(
        plus_interval(today(), "6 hours"),
        plus_interval(today(), "17 hours"),
        "15 minutes",
    );

    let events = query::QuerySource::Query(
        Arc::new(Box::new(
            events.get_select_query_for_field_names(&["start_time", "end_time"]),
        )),
        Some("e".to_string()),
    );

    Query::new()
        .with_with(
            "time_slots",
            Query::new().with_field("slot".to_string(), generate_series),
        )
        .with_table("time_slots", Some("ts".to_string()))
        .with_column_field("ts.slot")
        .with_join(JoinQuery::new(
            query::JoinType::Left,
            events,
            QueryConditions::on()
                .with_condition(expr!("ts.slot < e.end_time"))
                .with_condition(expr!("ts.slot + INTERVAL '1 hour' > e.start_time")),
        ))
        .with_group_by(expr!("ts.slot"))
        .with_having_condition(expr!("count(e.id) = 0"))

    // let query_time_series = Query::new()
    //     .with_source(query::QuerySource::Expression(
    //         ExpressionArc::fx(
    //             "GENERATE_SERIES",
    //             vec![
    //                 plus_interval(today(), "6 hours"),
    //                 plus_interval(today(), "17 hours"),
    //                 expr!("{}", "15 minutes")
    //             ],
    //         )
    //         .render_chunk(),
    //         Some("dates".to_string()),
    //     ))
    //     .with_field(
    //         "date".to_string(),
    //         expr!("date_trunc({}, dates)", "week".to_string()),
    //     );
}

#[tokio::main]
async fn main() -> Result<()> {
    create_bootstrap_db().await?;

    let person = Table::new("person", postgres())
        .with_id_column("id")
        .with_column("name");

    let events = Table::new("event", postgres())
        .with_id_column("id")
        .with_column("start_time")
        .with_column("end_time")
        .with_one("person", "person_id", move || Box::new(person.clone()));

    let q = find_available_slots("start_time", events);
    print!("{}", format_query(&q));

    Ok(())
}
