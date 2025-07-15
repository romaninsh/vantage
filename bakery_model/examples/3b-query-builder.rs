use vantage_expressions::{expr, OwnedExpression};
use vantage_sql::{
    select::{join_query::JoinQuery, query_source::QuerySource},
    Select,
};

use anyhow::Result;

use sqlformat::FormatOptions;
use sqlformat::QueryParams;

pub fn format_query(q: &Select) -> String {
    let select_expr: OwnedExpression = q.clone().into();
    let raw_sql = select_expr.preview();

    let formatted_sql = sqlformat::format(&raw_sql, &QueryParams::None, &FormatOptions::default());

    formatted_sql
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
    let github_authors_and_teams = Select::new()
        .with_table_alias("dx_teams", "t")
        .with_field("team_source_id", expr!("t.source_id"));

    // Team is an anchestor
    let github_authors_and_teams = github_authors_and_teams.with_join(
        JoinQuery::inner(QuerySource::table_with_alias("dx_team_hierarchies", "h"))
            .on(expr!("t.id = h.ancestor_id")),
    );

    // to a user with `user_name`
    let github_authors_and_teams = github_authors_and_teams
        .with_join(
            JoinQuery::inner(QuerySource::table_with_alias("dx_users", "dxu"))
                .on(expr!("h.descendant_id = dxu.team_id")),
        )
        .with_field("user_name", expr!("dxu.name"))
        .with_field("github_username", expr!("dxu.source_id"));

    // pin identity of a user
    let github_authors_and_teams = github_authors_and_teams
        .with_join(
            JoinQuery::inner(QuerySource::table_with_alias("identities", "i"))
                .on(expr!("dxu.id = i.dx_user_id"))
                .on(expr!("i.source = {}", "github")),
        )
        .with_field("user_source_id", expr!("i.source_id"));

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
    let query_successful_deployments = Select::new()
        .with_table("deployments")
        .with_distinct()
        .with_field("id", expr!("deployments.id"))
        .with_field("deployed_at", expr!("deployments.deployed_at"))
        .with_where_condition(expr!("deployments.success = {}", true))
        .with_where_condition(expr!("deployments.environment ~* {}", "prod"));

    // Service to where the deployment has taken place
    let query_successful_deployments = query_successful_deployments.with_join(
        JoinQuery::left(QuerySource::table("service_identities"))
            .on(expr!(
                "service_identities.source_id::numeric = deployments.deployment_service_id"
            ))
            .on(expr!("service_identities.source = {}", "deployments")),
    );

    // Service associations with the teams
    let query_successful_deployments = query_successful_deployments.with_join(
        JoinQuery::left(QuerySource::table("services"))
            .on(expr!("services.id = service_identities.service_id")),
    );

    // Deployment Pull contains environment details as well as pull IDs for our deployment
    let query_successful_deployments = query_successful_deployments.with_join(
        JoinQuery::left(QuerySource::table_with_alias(
            "github_pull_deployments",
            "gpd",
        ))
        .on(expr!("gpd.deployment_id = deployments.id")),
    );

    // Grabbing more information about pipeline execution
    let query_successful_deployments = query_successful_deployments.with_join(
        JoinQuery::left(QuerySource::table_with_alias("pipeline_runs", "piper"))
            .on(expr!("piper.commit_sha = deployments.commit_sha")),
    );

    // Fetch author information from a sub-query
    let query_successful_deployments = query_successful_deployments.with_join(
        JoinQuery::left(QuerySource::query_with_alias(
            github_authors_and_teams,
            "authors",
        ))
        .on(expr!(
            "LOWER(authors.github_username) = LOWER(piper.github_username)"
        )),
    );

    // We are only interested in a single team
    let query_successful_deployments = query_successful_deployments
        .with_where_condition(expr!("authors.team_source_id IN ({})", "NzM0MA"));

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

    let query_time_series = Select::new()
        .with_source(QuerySource::expression_with_alias(
            expr!(
                "GENERATE_SERIES({}::date, {}::date, {}::interval)",
                "2024-01-01",
                "2024-05-19",
                "1 week"
            ),
            "dates",
        ))
        .with_field("date", expr!("date_trunc({}, dates)", "week"));

    let deploys_deploys = Select::new()
        .with_table("time_series")
        .with_join(
            JoinQuery::left(QuerySource::query_with_alias(query_successful_deployments, "deploys"))
                .on(expr!(
                    "date_trunc({}, deploys.deployed_at) BETWEEN time_series.date AND time_series.date + INTERVAL '7 days'",
                    "day"
                ))
        )
        .with_field("date", expr!("time_series.date"))
        .with_field("deploys_count", expr!("COUNT(DISTINCT deploys.id)"))
        .with_group_by(expr!("time_series.date"))
        .with_order_by(expr!("time_series.date"));

    let final_query = Select::new()
        .with_with("time_series", query_time_series)
        .with_with("daily_deploys", deploys_deploys)
        .with_table("daily_deploys")
        .with_field("date", expr!("date"))
        .with_field("value", expr!("(SUM(daily_deploys.deploys_count) / 7)"))
        .with_group_by(expr!("date"))
        .with_order_by(expr!("date"));

    println!("=============================================================");
    println!("{}", format_query(&final_query));

    Ok(())
}
