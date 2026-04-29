//! Convert AWS Query XML responses into `serde_json::Value`.
//!
//! AWS Query responses always have the shape:
//!
//! ```xml
//! <{Action}Response xmlns="…">
//!   <{Action}Result>
//!     ... payload ...
//!   </{Action}Result>
//!   <ResponseMetadata>
//!     <RequestId>…</RequestId>
//!   </ResponseMetadata>
//! </{Action}Response>
//! ```
//!
//! We strip the outer two wrappers and return the inner payload as a
//! `JsonValue`. Within the payload, repeated `<member>` children mean
//! "this element is a list" — we hoist them to a JSON array. Other
//! elements with named children become JSON objects; leaf elements
//! become strings (no type coercion in v0).
//!
//! Attributes and namespaces are ignored — the Query protocol payload
//! shape doesn't lean on them.

use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::{Map, Value as JsonValue};
use vantage_core::{Result, error};

/// Parse an AWS Query XML response, returning the payload that lives
/// inside `<{Action}Result>`. The outer `{Action}Response` and inner
/// `{Action}Result` wrappers are stripped — the caller sees just the
/// fields the operation produced (e.g. `{"Users": [...], "IsTruncated": "false"}`).
///
/// `ResponseMetadata` is dropped. We don't need the request id at this
/// layer (errors surface in the HTTP transport with the full body
/// already).
pub(crate) fn parse_query_response(xml: &str) -> Result<JsonValue> {
    let root = parse_root_element(xml)?;
    // The root is `<{Action}Response>`. Find the `{Action}Result`
    // child and return its contents. If there isn't one (e.g. an
    // operation with no payload), fall back to the response element
    // itself minus `ResponseMetadata`.
    match root {
        XmlNode::Element { name: _, children } => {
            for child in &children {
                if let XmlNode::Element { name, children: c2 } = child
                    && name.ends_with("Result")
                {
                    return Ok(element_children_to_json(c2));
                }
            }
            // No `*Result` element — return the whole thing minus metadata.
            let kept: Vec<&XmlNode> = children
                .iter()
                .filter(|c| match c {
                    XmlNode::Element { name, .. } => name != "ResponseMetadata",
                    _ => true,
                })
                .collect();
            Ok(nodes_to_json(&kept))
        }
        XmlNode::Text(t) => Ok(JsonValue::String(t)),
    }
}

#[derive(Debug)]
enum XmlNode {
    Element {
        name: String,
        children: Vec<XmlNode>,
    },
    Text(String),
}

fn parse_root_element(xml: &str) -> Result<XmlNode> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    // Skip the prologue (XML decl, comments, whitespace) until we hit
    // the first start element — that's the root.
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_name(e.name().as_ref());
                let children = read_children(&mut reader)?;
                return Ok(XmlNode::Element { name, children });
            }
            Ok(Event::Empty(e)) => {
                let name = local_name(e.name().as_ref());
                return Ok(XmlNode::Element {
                    name,
                    children: Vec::new(),
                });
            }
            Ok(Event::Eof) => {
                return Err(error!("AWS Query XML response is empty"));
            }
            Ok(_) => continue,
            Err(e) => {
                return Err(error!("Failed to parse AWS Query XML response", detail = e));
            }
        }
    }
}

/// Read children of the currently-open element until the matching End
/// event. Caller must have just consumed the Start event for the
/// parent.
fn read_children(reader: &mut Reader<&[u8]>) -> Result<Vec<XmlNode>> {
    let mut children = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_name(e.name().as_ref());
                let nested = read_children(reader)?;
                children.push(XmlNode::Element {
                    name,
                    children: nested,
                });
            }
            Ok(Event::Empty(e)) => {
                let name = local_name(e.name().as_ref());
                children.push(XmlNode::Element {
                    name,
                    children: Vec::new(),
                });
            }
            Ok(Event::Text(t)) => {
                let s = t
                    .unescape()
                    .map_err(|e| error!("XML text decode failed", detail = e))?
                    .into_owned();
                if !s.is_empty() {
                    children.push(XmlNode::Text(s));
                }
            }
            Ok(Event::CData(t)) => {
                children.push(XmlNode::Text(String::from_utf8_lossy(t.as_ref()).into_owned()));
            }
            Ok(Event::End(_)) => return Ok(children),
            Ok(Event::Eof) => {
                return Err(error!("AWS Query XML ended mid-element"));
            }
            Ok(_) => continue,
            Err(e) => {
                return Err(error!("Failed to parse AWS Query XML", detail = e));
            }
        }
    }
}

fn local_name(qname: &[u8]) -> String {
    let s = std::str::from_utf8(qname).unwrap_or("");
    match s.split_once(':') {
        Some((_, local)) => local.to_string(),
        None => s.to_string(),
    }
}

/// Convert the children of an element into a JSON value.
///
/// - All children named `member` → JSON array of converted children.
/// - All children are text → concatenated text as `JsonValue::String`.
/// - Otherwise → JSON object keyed by child element name. Repeated
///   keys collapse into a JSON array (rare outside `<member>` lists,
///   but defensive).
fn element_children_to_json(children: &[XmlNode]) -> JsonValue {
    let refs: Vec<&XmlNode> = children.iter().collect();
    nodes_to_json(&refs)
}

fn nodes_to_json(children: &[&XmlNode]) -> JsonValue {
    let only_text = !children.is_empty()
        && children.iter().all(|n| matches!(n, XmlNode::Text(_)));
    if only_text {
        let mut s = String::new();
        for n in children {
            if let XmlNode::Text(t) = n {
                s.push_str(t);
            }
        }
        return JsonValue::String(s);
    }

    // Drop interleaved whitespace text nodes when there are real
    // element siblings (we already stripped pure whitespace via
    // `trim_text`, but mixed content shouldn't appear in AWS payloads
    // anyway).
    let elements: Vec<&XmlNode> = children
        .iter()
        .copied()
        .filter(|n| matches!(n, XmlNode::Element { .. }))
        .collect();

    if elements.is_empty() {
        return JsonValue::String(String::new());
    }

    let all_member = elements.iter().all(|n| matches!(n, XmlNode::Element { name, .. } if name == "member"));
    if all_member {
        let arr = elements
            .iter()
            .map(|n| match n {
                XmlNode::Element { children, .. } => element_children_to_json(children),
                _ => unreachable!(),
            })
            .collect();
        return JsonValue::Array(arr);
    }

    let mut map = Map::new();
    for n in &elements {
        if let XmlNode::Element { name, children } = n {
            let v = element_children_to_json(children);
            match map.remove(name) {
                None => {
                    map.insert(name.clone(), v);
                }
                Some(existing) => {
                    let arr = match existing {
                        JsonValue::Array(mut a) => {
                            a.push(v);
                            a
                        }
                        other => vec![other, v],
                    };
                    map.insert(name.clone(), JsonValue::Array(arr));
                }
            }
        }
    }
    JsonValue::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn list_users_response_round_trip() {
        let xml = r#"<?xml version="1.0"?>
<ListUsersResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">
  <ListUsersResult>
    <Users>
      <member>
        <UserName>Alice</UserName>
        <UserId>AIDAALICE</UserId>
        <Path>/</Path>
        <Arn>arn:aws:iam::123:user/Alice</Arn>
        <CreateDate>2020-01-01T00:00:00Z</CreateDate>
      </member>
      <member>
        <UserName>Bob</UserName>
        <UserId>AIDABOB</UserId>
        <Path>/admin/</Path>
        <Arn>arn:aws:iam::123:user/Bob</Arn>
        <CreateDate>2021-06-01T00:00:00Z</CreateDate>
      </member>
    </Users>
    <IsTruncated>false</IsTruncated>
  </ListUsersResult>
  <ResponseMetadata>
    <RequestId>abc-123</RequestId>
  </ResponseMetadata>
</ListUsersResponse>"#;

        let v = parse_query_response(xml).unwrap();
        assert_eq!(
            v,
            json!({
                "Users": [
                    {
                        "UserName": "Alice",
                        "UserId": "AIDAALICE",
                        "Path": "/",
                        "Arn": "arn:aws:iam::123:user/Alice",
                        "CreateDate": "2020-01-01T00:00:00Z",
                    },
                    {
                        "UserName": "Bob",
                        "UserId": "AIDABOB",
                        "Path": "/admin/",
                        "Arn": "arn:aws:iam::123:user/Bob",
                        "CreateDate": "2021-06-01T00:00:00Z",
                    }
                ],
                "IsTruncated": "false",
            })
        );
    }

    #[test]
    fn empty_member_list_becomes_empty_string() {
        // `<Users/>` is a self-closing element with no children — we
        // surface it as the empty string here (we don't know it's a
        // list without seeing a `<member>`). `query::parse_records`
        // is the layer that knows array_key is meant to hold rows and
        // promotes "" to an empty array there.
        let xml = r#"<ListUsersResponse>
  <ListUsersResult>
    <Users/>
    <IsTruncated>false</IsTruncated>
  </ListUsersResult>
</ListUsersResponse>"#;
        let v = parse_query_response(xml).unwrap();
        assert_eq!(v["Users"], json!(""));
    }
}
