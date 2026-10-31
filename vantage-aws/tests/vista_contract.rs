//! `vantage_vista::contract` over live IAM users.
//!
//! Opt-in: skipped (returns early) unless `VANTAGE_AWS_LIVE=1` is set and
//! AWS credentials are configured. Having `~/.aws/credentials` alone never
//! runs it. Read-only: the backend advertises no writes. The transactions
//! contract also runs offline, against a loopback IAM endpoint.

use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use vantage_aws::AwsAccount;
use vantage_aws::models::iam;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_vista::contract::{TxFixture, TxTables};
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

/// The IAM API has no transactions, and the backend takes no writes. Both
/// tables are the users: the probe reads only the first.
struct Account(Vista);

#[async_trait]
impl TxFixture for Account {
    async fn fresh(&self) -> TxTables {
        TxTables::new(self.0.clone(), self.0.clone())
    }

    async fn fresh_pool_of_one(&self) -> TxTables {
        self.fresh().await
    }

    fn detects_deadlocks(&self) -> bool {
        false
    }

    fn supports_transactions(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn aws_transactions_hold_the_contract() {
    let Some(users) = users_or_skip() else {
        return;
    };
    contract::check_transactions(&Account(users)).await
}

const LIST_USERS: &str = r#"<?xml version="1.0"?>
<ListUsersResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">
  <ListUsersResult>
    <Users>
      <member>
        <UserName>alice</UserName>
        <UserId>AIDAALICE</UserId>
        <Path>/</Path>
        <Arn>arn:aws:iam::123:user/alice</Arn>
        <CreateDate>2020-01-01T00:00:00Z</CreateDate>
      </member>
      <member>
        <UserName>bob</UserName>
        <UserId>AIDABOB</UserId>
        <Path>/</Path>
        <Arn>arn:aws:iam::123:user/bob</Arn>
        <CreateDate>2020-01-02T00:00:00Z</CreateDate>
      </member>
    </Users>
    <IsTruncated>false</IsTruncated>
  </ListUsersResult>
</ListUsersResponse>"#;

async fn reply_list_users(stream: TcpStream) {
    let mut reader = BufReader::new(stream);
    let mut length = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.unwrap() == 0 {
            return;
        }
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap();
        }
    }
    reader.read_exact(&mut vec![0; length]).await.unwrap();
    let reply = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/xml\r\n\
         content-length: {}\r\nconnection: close\r\n\r\n{LIST_USERS}",
        LIST_USERS.len()
    );
    reader.get_mut().write_all(reply.as_bytes()).await.unwrap();
}

/// Answers every request on 127.0.0.1 with the same two IAM users.
async fn loopback_iam() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(reply_list_users(stream));
        }
    });
    url
}

#[tokio::test]
async fn aws_transactions_hold_the_contract_offline() {
    let aws = AwsAccount::new("AKID", "secret", "us-east-1").with_endpoint(loopback_iam().await);
    let users = aws
        .vista_factory()
        .from_table(iam::users_table(aws.clone()))
        .unwrap();
    timeout(
        Duration::from_secs(4),
        contract::check_transactions(&Account(users)),
    )
    .await
    .expect("the offline transactions contract ran out of time")
}
