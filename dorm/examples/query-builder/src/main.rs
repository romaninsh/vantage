use std::sync::Arc;

use dorm::prelude::*;
use dorm::query;
use serde_json::json;
use tokio_postgres::NoTls;

use anyhow::Result;

mod formatter;

extern crate dorm;

#[tokio::main]
async fn main() -> Result<()> {
    let (client, connection) =
        tokio_postgres::connect("host=localhost dbname=postgres", NoTls).await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    let postgres = Postgres::new(Arc::new(Box::new(client)));

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
        .set_table("dx_teams", Some("t".to_string()))
        .add_column("team_source_id".to_string(), expr!("t.source_id"));

    // Team is an anchestor
    let github_authors_and_teams = github_authors_and_teams.add_join(query::JoinQuery::new(
        query::JoinType::Inner,
        query::QuerySource::Table("dx_team_hierarchies".to_string(), Some("h".to_string())),
        query::QueryConditions::on().add_condition(expr!("t.id = h.ancestor_id")),
    ));

    // to a user with `user_name`
    let github_authors_and_teams = github_authors_and_teams
        .add_join(query::JoinQuery::new(
            query::JoinType::Inner,
            query::QuerySource::Table("dx_users".to_string(), Some("dxu".to_string())),
            query::QueryConditions::on().add_condition(expr!("h.descendant_id = dxu.team_id")),
        ))
        .add_column("user_name".to_string(), expr!("dxu.name"))
        .add_column("github_username".to_string(), expr!("dxu.source_id"));

    // pin identity of a user
    let github_authors_and_teams = github_authors_and_teams
        .add_join(query::JoinQuery::new(
            query::JoinType::Inner,
            query::QuerySource::Table("identities".to_string(), Some("i".to_string())),
            query::QueryConditions::on()
                .add_condition(expr!("dxu.id = i.dx_user_id"))
                .add_condition(expr!("i.source = {}", "github")),
        ))
        .add_column("user_source_id".to_string(), expr!("i.source_id"));

    println!("{}", formatter::format_query(&github_authors_and_teams));

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
        .set_table("deployments", None)
        .distinct()
        .add_column("id".to_string(), expr!("deployments.id"))
        .add_column("deployed_at".to_string(), expr!("deployments.deployed_at"))
        .add_condition(expr!("deployments.success = {}", true))
        .add_condition(expr!("deployments.environment ~* {}", "prod"));

    // Service to where the deployment has taken place
    let query_successful_deployments =
        query_successful_deployments.add_join(query::JoinQuery::new(
            query::JoinType::Left,
            query::QuerySource::Table("service_identities".to_string(), None),
            query::QueryConditions::on()
                .add_condition(expr!(
                    "service_identities.source_id::numeric = deployments.deployment_service_id"
                ))
                .add_condition(expr!("service_identities.source = {}", "deployments")),
        ));

    // Service associations with the teams
    let query_successful_deployments =
        query_successful_deployments.add_join(query::JoinQuery::new(
            query::JoinType::Left,
            query::QuerySource::Table("services".to_string(), None),
            query::QueryConditions::on()
                .add_condition(expr!("services.id = service_identities.service_id")),
        ));

    // Deployment Pull contains environment details as well as pull IDs for our deployment
    let query_successful_deployments =
        query_successful_deployments.add_join(query::JoinQuery::new(
            query::JoinType::Left,
            query::QuerySource::Table(
                "github_pull_deployments".to_string(),
                Some("gpd".to_string()),
            ),
            query::QueryConditions::on().add_condition(expr!("gpd.deployment_id = deployments.id")),
        ));

    // Grabbing more information about pipeline execution
    let query_successful_deployments =
        query_successful_deployments.add_join(query::JoinQuery::new(
            query::JoinType::Left,
            query::QuerySource::Table("pipeline_runs".to_string(), Some("piper".to_string())),
            query::QueryConditions::on()
                .add_condition(expr!("piper.commit_sha = deployments.commit_sha")),
        ));

    // Fetch author information from a sub-query
    let query_successful_deployments =
        query_successful_deployments.add_join(query::JoinQuery::new(
            query::JoinType::Left,
            query::QuerySource::Query(
                Arc::new(Box::new(github_authors_and_teams)),
                Some("authors".to_string()),
            ),
            query::QueryConditions::on().add_condition(expr!(
                "LOWER(authors.github_username) = LOWER(piper.github_username)"
            )),
        ));

    // We are only interested in a single team
    let query_successful_deployments = query_successful_deployments
        .add_condition(expr!("authors.team_source_id IN ({})", "NzM0MA"));

    println!("=============================================================");
    println!("{}", formatter::format_query(&query_successful_deployments));

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
        .set_source(query::QuerySource::Expression(
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
        .add_column(
            "date".to_string(),
            expr!("date_trunc({}, dates)", "week".to_string()),
        );

    let deploys_deploys = Query::new()
        .set_table("time_series", None)
        .add_join(query::JoinQuery::new(
            query::JoinType::Left,
            query::QuerySource::Query(Arc::new(Box::new(query_successful_deployments)), Some("deploys".to_string())),
            query::QueryConditions::on()
                .add_condition(expr!(
                    "date_trunc({}, deploys.deployed_at) BETWEEN time_series.date AND time_series.date + INTERVAL '7 days'",
                    "day".to_string()
                )),
        )).add_column("date".to_string(), expr!("time_series.date"))
        .add_column("deploys_count".to_string(), expr!("COUNT(DISTINCT deploys.id)"))
        .add_group_by(expr!("time_series.date")).add_order_by(expr!("time_series.date"));

    let final_query = Query::new()
        .add_with("time_series", query_time_series)
        .add_with("daily_deploys", deploys_deploys)
        .set_table("daily_deploys", None)
        .add_column("date".to_string(), expr!("date"))
        .add_column(
            "value".to_string(),
            expr!("(SUM(daily_deploys.deploys_count) / 7)"),
        )
        .add_group_by(expr!("date"))
        .add_order_by(expr!("date"));

    println!("=============================================================");
    println!("{}", formatter::format_query(&final_query));

    Ok(())
}