//! Entity and character references in XML responses decode the same way
//! for both XML protocols: REST-XML (S3) and Query (IAM). Each test runs
//! against a one-shot loopback server, so no request leaves the machine.

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use vantage_aws::AwsAccount;
use vantage_aws::models::{iam, s3};
use vantage_aws::prelude::ReadableDataSet;

/// Serve one HTTP request on 127.0.0.1 with `body` as the XML reply, and
/// return the endpoint URL.
async fn serve_once(body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
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
            "HTTP/1.1 200 OK\r\ncontent-type: application/xml\r\n\
             content-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        reader.get_mut().write_all(reply.as_bytes()).await.unwrap();
    });
    url
}

#[tokio::test]
async fn restxml_list_buckets_decodes_entities() {
    let endpoint = serve_once(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
   <Buckets>
      <Bucket>
         <Name>Tom &amp; Jerry</Name>
         <CreationDate>2024-01-01T00:00:00Z</CreationDate>
      </Bucket>
      <Bucket>
         <Name>  a&lt;b&gt;c &#38; d&#x26;e  </Name>
         <CreationDate>
            2024-02-01T00:00:00Z
         </CreationDate>
      </Bucket>
   </Buckets>
</ListAllMyBucketsResult>"#,
    )
    .await;
    let aws = AwsAccount::new("AKID", "secret", "us-east-1").with_endpoint(endpoint);

    let buckets = s3::buckets_table(aws).list().await.unwrap();

    let names: Vec<&str> = buckets.keys().map(|k| k.as_str()).collect();
    assert_eq!(names, ["Tom & Jerry", "a<b>c & d&e"]);
    assert_eq!(buckets["Tom & Jerry"].name, "Tom & Jerry");
    assert_eq!(buckets["a<b>c & d&e"].name, "a<b>c & d&e");
    assert_eq!(buckets["a<b>c & d&e"].creation_date, "2024-02-01T00:00:00Z");
}

#[tokio::test]
async fn query_list_users_decodes_entities() {
    let endpoint = serve_once(
        r#"<?xml version="1.0"?>
<ListUsersResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">
  <ListUsersResult>
    <Users>
      <member>
        <UserName>Tom &amp; Jerry</UserName>
        <UserId>  AIDATOM  </UserId>
        <Path>/a&lt;b&gt;c/</Path>
        <Arn>arn:aws:iam::123:user/x&#38;y</Arn>
        <CreateDate>2020-01-01T00:00:00Z</CreateDate>
      </member>
    </Users>
    <IsTruncated>false</IsTruncated>
  </ListUsersResult>
  <ResponseMetadata>
    <RequestId>abc-123</RequestId>
  </ResponseMetadata>
</ListUsersResponse>"#,
    )
    .await;
    let aws = AwsAccount::new("AKID", "secret", "us-east-1").with_endpoint(endpoint);

    let users = iam::users_table(aws).list().await.unwrap();

    assert_eq!(users.len(), 1);
    let user = &users["Tom & Jerry"];
    assert_eq!(user.user_name, "Tom & Jerry");
    assert_eq!(user.user_id, "AIDATOM");
    assert_eq!(user.path, "/a<b>c/");
    assert_eq!(user.arn, "arn:aws:iam::123:user/x&y");
    assert_eq!(user.create_date, "2020-01-01T00:00:00Z");
}
