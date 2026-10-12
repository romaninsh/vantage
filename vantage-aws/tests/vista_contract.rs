//! `vantage_vista::contract` over live IAM users.
//!
//! Opt-in: skipped (returns early) unless `VANTAGE_AWS_LIVE=1` is set and
//! AWS credentials are configured. Having `~/.aws/credentials` alone never
//! runs it. Read-only: the backend advertises no writes.

use vantage_aws::AwsAccount;
use vantage_aws::models::iam;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_vista::{CborValue, Vista, contract};

fn users_or_skip() -> Option<Vista> {
    if std::env::var("VANTAGE_AWS_LIVE").as_deref() != Ok("1") {
        eprintln!("skipping: set VANTAGE_AWS_LIVE=1 to run against real AWS");
        return None;
    }
    let Ok(aws) = AwsAccount::from_default() else {
        eprintln!("skipping: AWS credentials not configured");
        return None;
    };
    Some(
        aws.vista_factory()
            .from_table(iam::users_table(aws.clone()))
            .unwrap(),
    )
}

#[tokio::test]
async fn aws_contract_advertised() {
    let Some(users) = users_or_skip() else {
        return;
    };
    contract::check_advertised(&users).await;
}

#[tokio::test]
async fn aws_contract_clone() {
    let Some(users) = users_or_skip() else {
        return;
    };
    // Narrow to one user by name; that only drops rows when there are two.
    let names: Vec<String> = users.list_values().await.unwrap().into_keys().collect();
    if names.len() < 2 {
        eprintln!("skipping: the account needs at least two IAM users");
        return;
    }
    contract::check_clone(&users, "UserName", CborValue::Text(names[0].clone())).await;
}
