//! The leading window over a one-shot loopback server standing in for
//! CloudWatch Logs, so no request leaves the machine.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

use crate::models::logs;

/// Serve one HTTP request on 127.0.0.1 with `body` as the JSON reply, and
/// return the endpoint URL.
fn serve_once(body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse().unwrap();
            }
        }
        reader.read_exact(&mut vec![0; length]).unwrap();
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/x-amz-json-1.1\r\n\
             content-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        reader.get_mut().write_all(reply.as_bytes()).unwrap();
    });
    url
}

#[tokio::test]
async fn the_leading_window_is_cut_from_one_listing() {
    let endpoint = serve_once(
        r#"{"logGroups":[{"logGroupName":"a"},{"logGroupName":"b"},{"logGroupName":"c"}]}"#,
    );
    let aws = super::aws().with_endpoint(endpoint);
    let vista = aws
        .vista_factory()
        .from_table(logs::groups_table(aws.clone()))
        .unwrap();

    let page = vista.fetch_window(0, 2).await.unwrap();
    let ids: Vec<&str> = page.rows.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(page.total, Some(3));
}
