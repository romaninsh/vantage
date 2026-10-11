//! `vantage_vista::contract` over live IAM users.
//!
//! Skipped (returns early) when AWS credentials aren't configured, so CI
//! without AWS access stays green. Read-only: the backend advertises no
//! writes.

use vantage_aws::AwsAccount;
use vantage_aws::models::iam;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_vista::{CborValue, Vista, contract};

fn users_or_skip() -> Option<Vista> {
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
