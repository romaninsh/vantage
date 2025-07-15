use std::sync::Arc;

use serde_json::json;
use vantage::prelude::*;
use vantage::sql::query;

use anyhow::Result;

use sqlformat::FormatOptions;
use sqlformat::QueryParams;
// use syntect::easy::HighlightLines;
// use syntect::highlighting::Style;
// use syntect::highlighting::ThemeSet;
// use syntect::parsing::SyntaxSet;
// use syntect::util::as_24_bit_terminal_escaped;
// use syntect::util::LinesWithEndings;

extern crate vantage;

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

#[tokio::main]
async fn main() -> Result<()> {
    // Let start with the simpler query
    // SELECT i.source_id AS user_source_id,
    //   dxu.name AS user_name,
    //   t.source_id AS team_source_id,
    //   dxu.github_username
    // FROM dx_teams t
    //   JOIN dx_team_hierarchies h ON t.id = h.ancestor_id
    //   JOIN dx_users dxu ON h.descendant_id = dxu.team_id
    //   JOIN identities i ON dxu.id = i.dx_user_id
    //   AND i.source = 'github'

    // Associate Github Authors (github_username, user_name) with theirTeam IDs (user_source_id, team_source_id)
    let github_authors_and_teams = Query::new()
        .with_table("dx_teams", Some("t".to_string()))
        .with_field("team_source_id".to_string(), expr!("t.source_id"));

    // Team is an anchestor
    let github_authors_and_teams = github_authors_and_teams.with_join(query::JoinQuery::new(
        query::JoinType::Inner,
        query::QuerySource::Table("dx_team_hierarchies".to_string(), Some("h".to_string())),
        query::QueryConditions::on().with_condition(expr!("t.id = h.ancestor_id")),
    ));

    // to a user with `user_name`
    let github_authors_and_teams = github_authors_and_teams
        .with_join(query::JoinQuery::new(
            query::JoinType::Inner,
            query::QuerySource::Table("dx_users".to_string(), Some("dxu".to_string())),
            query::QueryConditions::on().with_condition(expr!("h.descendant_id = dxu.team_id")),
        ))
        .with_field("user_name".to_string(), expr!("dxu.name"))
        .with_field("github_username".to_string(), expr!("dxu.source_id"));

    // pin identity of a user
    let github_authors_and_teams = github_authors_and_teams
        .with_join(query::JoinQuery::new(
            query::JoinType::Inner,
            query::QuerySource::Table("identities".to_string(), Some("i".to_string())),
            query::QueryConditions::on()
                .with_condition(expr!("dxu.id = i.dx_user_id"))
                .with_condition(expr!("i.source = {}", "github")),
        ))
        .with_field("user_source_id".to_string(), expr!("i.source_id"));

    println!("{}", format_query(&github_authors_and_teams));

    // SELECT DISTINCT deployments.id,
    //   deployments.deployed_at
    // FROM deployments
    //   LEFT JOIN service_identities ON service_identities.source_id::numeric = deployments.deployment_service_id
    //   AND service_identities.source = 'deployments'
    //   LEFT JOIN services ON services.id = service_identities.service_id
    //   LEFT JOIN github_pull_deployments AS gpd ON gpd.deployment_id = deployments.id
    //   LEFT JOIN pipeline_runs AS piper ON piper.commit_sha = deployments.commit_sha
    //   LEFT JOIN (
    //     SELECT i.source_id AS user_source_id,
    //       dxu.name AS user_name,
    //       t.source_id AS team_source_id,
    //       dxu.github_username
    //     FROM dx_teams t
    //       JOIN dx_team_hierarchies h ON t.id = h.ancestor_id
    //       JOIN dx_users dxu ON h.descendant_id = dxu.team_id
    //       JOIN identities i ON dxu.id = i.dx_user_id
    //       AND i.source = 'github'
    //   ) AS authors ON LOWER(authors.github_username) = LOWER(piper.github_username)
    // WHERE deployments.success = true
    //   AND (deployments.environment ~* 'prod')
    //   AND authors.team_source_id IN ('NzM0MA')  ) as dates

    // Start by querying all deployments
    let query_successful_deployments = Query::new()
        .with_table("deployments", None)
        .with_distinct()
        .with_field("id".to_string(), expr!("deployments.id"))
        .with_field("deployed_at".to_string(), expr!("deployments.deployed_at"))
        .with_condition(expr!("deployments.success = {}", true))
        .with_condition(expr!("deployments.environment ~* {}", "prod"));

    // Service to where the deployment has taken place
    let query_successful_deployments =
        query_successful_deployments.with_join(query::JoinQuery::new(
            query::JoinType::Left,
            query::QuerySource::Table("service_identities".to_string(), None),
            query::QueryConditions::on()
                .with_condition(expr!(
                    "service_identities.source_id::numeric = deployments.deployment_service_id"
                ))
                .with_condition(expr!("service_identities.source = {}", "deployments")),
        ));

    // Service associations with the teams
    let query_successful_deployments =
        query_successful_deployments.with_join(query::JoinQuery::new(
            query::JoinType::Left,
            query::QuerySource::Table("services".to_string(), None),
            query::QueryConditions::on()
                .with_condition(expr!("services.id = service_identities.service_id")),
        ));

    // Deployment Pull contains environment details as well as pull IDs for our deployment
    let query_successful_deployments =
        query_successful_deployments.with_join(query::JoinQuery::new(
            query::JoinType::Left,
            query::QuerySource::Table(
                "github_pull_deployments".to_string(),
                Some("gpd".to_string()),
            ),
            query::QueryConditions::on()
                .with_condition(expr!("gpd.deployment_id = deployments.id")),
        ));

    // Grabbing more information about pipeline execution
    let query_successful_deployments =
        query_successful_deployments.with_join(query::JoinQuery::new(
            query::JoinType::Left,
            query::QuerySource::Table("pipeline_runs".to_string(), Some("piper".to_string())),
            query::QueryConditions::on()
                .with_condition(expr!("piper.commit_sha = deployments.commit_sha")),
        ));

    // Fetch author information from a sub-query
    let query_successful_deployments =
        query_successful_deployments.with_join(query::JoinQuery::new(
            query::JoinType::Left,
            query::QuerySource::Query(
                Arc::new(Box::new(github_authors_and_teams)),
                Some("authors".to_string()),
            ),
            query::QueryConditions::on().with_condition(expr!(
                "LOWER(authors.github_username) = LOWER(piper.github_username)"
            )),
        ));

    // We are only interested in a single team
    let query_successful_deployments = query_successful_deployments
        .with_condition(expr!("authors.team_source_id IN ({})", "NzM0MA"));

    println!("=============================================================");
    println!("{}", format_query(&query_successful_deployments));

    // next wrap this up into a time series
    // WITH time_series AS (
    //   SELECT date_trunc('week', dates) as date
    //   FROM GENERATE_SERIES(
    //       '2024-01-01'::date,
    //       '2024-05-19'::date,
    //       '1 week'::interval
    //     ) as dates
    // ),
    // daily_deploys AS (
    //   SELECT time_series.date,
    //     COUNT(DISTINCT deploys.id) AS deploys_count
    //   FROM time_series
    //     LEFT JOIN (
    //       SELECT DISTINCT deployments.id,
    //         deployments.deployed_at
    //       FROM deployments
    //         LEFT JOIN service_identities ON service_identities.source_id::numeric = deployments.deployment_service_id
    //         AND service_identities.source = 'deployments'
    //         LEFT JOIN services ON services.id = service_identities.service_id
    //         LEFT JOIN github_pull_deployments AS gpd ON gpd.deployment_id = deployments.id
    // -- Instead of joining on pull request and pull request user
    // -- joining on the github_username from the pipeline run associated with the deployment
    //         LEFT JOIN pipeline_runs AS piper ON piper.commit_sha = deployments.commit_sha
    //         LEFT JOIN (
    //           SELECT i.source_id AS user_source_id,
    //             dxu.name AS user_name,
    //             t.source_id AS team_source_id,
    //             dxu.github_username
    //           FROM dx_teams t
    //             JOIN dx_team_hierarchies h ON t.id = h.ancestor_id
    //             JOIN dx_users dxu ON h.descendant_id = dxu.team_id
    //             JOIN identities i ON dxu.id = i.dx_user_id
    //             AND i.source = 'github'
    //         ) AS authors ON LOWER(authors.github_username) = LOWER(piper.github_username)
    //       WHERE deployments.success = true
    //         AND (deployments.environment ~* 'prod')
    //         AND authors.team_source_id IN ('NzM0MA')
    //     ) AS deploys ON date_trunc('day', deploys.deployed_at) BETWEEN time_series.date AND time_series.date + INTERVAL '7 days'
    //   GROUP BY time_series.date
    //   ORDER BY time_series.date
    // )
    // SELECT date,
    //   (SUM(daily_deploys.deploys_count) / 7) AS value
    // FROM daily_deploys
    // GROUP BY date
    // ORDER BY date

    let query_time_series = Query::new()
        .with_source(query::QuerySource::Expression(
            ExpressionArc::fx(
                "GENERATE_SERIES",
                vec![
                    Expression::as_type(json!("2024-01-01"), "date"),
                    Expression::as_type(json!("2024-05-19"), "date"),
                    Expression::as_type(json!("1 week"), "interval"),
                ],
            )
            .render_chunk(),
            Some("dates".to_string()),
        ))
        .with_field(
            "date".to_string(),
            expr!("date_trunc({}, dates)", "week".to_string()),
        );

    let deploys_deploys = Query::new()
        .with_table("time_series", None)
        .with_join(query::JoinQuery::new(
            query::JoinType::Left,
            query::QuerySource::Query(Arc::new(Box::new(query_successful_deployments)), Some("deploys".to_string())),
            query::QueryConditions::on()
                .with_condition(expr!(
                    "date_trunc({}, deploys.deployed_at) BETWEEN time_series.date AND time_series.date + INTERVAL '7 days'",
                    "day".to_string()
                )),
        )).with_field("date".to_string(), expr!("time_series.date"))
        .with_field("deploys_count".to_string(), expr!("COUNT(DISTINCT deploys.id)"))
        .with_group_by(expr!("time_series.date")).with_order_by(expr!("time_series.date"));

    let final_query = Query::new()
        .with_with("time_series", query_time_series)
        .with_with("daily_deploys", deploys_deploys)
        .with_table("daily_deploys", None)
        .with_field("date".to_string(), expr!("date"))
        .with_field(
            "value".to_string(),
            expr!("(SUM(daily_deploys.deploys_count) / 7)"),
        )
        .with_group_by(expr!("date"))
        .with_order_by(expr!("date"));

    println!("=============================================================");
    println!("{}", format_query(&final_query));

    Ok(())
}
