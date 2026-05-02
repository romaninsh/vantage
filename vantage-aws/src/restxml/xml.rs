//! REST-XML response normaliser.
//!
//! S3 (and other rest-xml services) skip the `<{Action}Response>` /
//! `<{Action}Result>` wrappers that the AWS Query protocol uses —
//! the response root *is* the result element. So this parser strips
//! the root element and surfaces its children as a JSON object,
//! collapsing repeated child names into JSON arrays.
//!
//! That lets callers reach into the response with a dotted lookup
//! like `Buckets.Bucket` for `ListBuckets`, or `Contents` for
//! `ListObjectsV2`.

use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::{Map, Value as JsonValue};
use vantage_core::{Result, error};

pub(crate) fn parse_xml_response(xml: &str) -> Result<JsonValue> {
    let root = parse_root_element(xml)?;
    match root {
        XmlNode::Element { children, .. } => {
            let refs: Vec<&XmlNode> = children.iter().collect();
            Ok(nodes_to_json(&refs))
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
                return Err(error!("AWS REST-XML response is empty"));
            }
            Ok(_) => continue,
            Err(e) => {
                return Err(error!("Failed to parse AWS REST-XML response", detail = e));
            }
        }
    }
}

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
                children.push(XmlNode::Text(
                    String::from_utf8_lossy(t.as_ref()).into_owned(),
                ));
            }
            Ok(Event::End(_)) => return Ok(children),
            Ok(Event::Eof) => {
                return Err(error!("AWS REST-XML ended mid-element"));
            }
            Ok(_) => continue,
            Err(e) => {
                return Err(error!("Failed to parse AWS REST-XML", detail = e));
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

fn element_children_to_json(children: &[XmlNode]) -> JsonValue {
    let refs: Vec<&XmlNode> = children.iter().collect();
    nodes_to_json(&refs)
}

fn nodes_to_json(children: &[&XmlNode]) -> JsonValue {
    let only_text = !children.is_empty() && children.iter().all(|n| matches!(n, XmlNode::Text(_)));
    if only_text {
        let mut s = String::new();
        for n in children {
            if let XmlNode::Text(t) = n {
                s.push_str(t);
            }
        }
        return JsonValue::String(s);
    }

    let elements: Vec<&XmlNode> = children
        .iter()
        .copied()
        .filter(|n| matches!(n, XmlNode::Element { .. }))
        .collect();

    if elements.is_empty() {
        return JsonValue::String(String::new());
    }

    // Same flattening rule as the Query parser, minus the `<member>`
    // shortcut: REST-XML names every element after its entity
    // (`<Bucket>`, `<Contents>`), so repeated children with the same
    // name collapse into a JSON array.
    let mut map: Map<String, JsonValue> = Map::new();
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
    fn list_buckets_response_round_trip() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
   <Owner>
      <ID>abc</ID>
      <DisplayName>name</DisplayName>
   </Owner>
   <Buckets>
      <Bucket>
         <Name>foo</Name>
         <CreationDate>2024-01-01T00:00:00Z</CreationDate>
      </Bucket>
      <Bucket>
         <Name>bar</Name>
         <CreationDate>2024-02-01T00:00:00Z</CreationDate>
      </Bucket>
   </Buckets>
</ListAllMyBucketsResult>"#;
        let v = parse_xml_response(xml).unwrap();
        assert_eq!(
            v,
            json!({
                "Owner": { "ID": "abc", "DisplayName": "name" },
                "Buckets": {
                    "Bucket": [
                        { "Name": "foo", "CreationDate": "2024-01-01T00:00:00Z" },
                        { "Name": "bar", "CreationDate": "2024-02-01T00:00:00Z" },
                    ]
                }
            })
        );
    }

    #[test]
    fn list_objects_v2_response_round_trip() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
   <Name>my-bucket</Name>
   <KeyCount>2</KeyCount>
   <Contents>
      <Key>a/b.txt</Key>
      <Size>10</Size>
      <ETag>"x"</ETag>
   </Contents>
   <Contents>
      <Key>a/c.txt</Key>
      <Size>20</Size>
      <ETag>"y"</ETag>
   </Contents>
</ListBucketResult>"#;
        let v = parse_xml_response(xml).unwrap();
        assert_eq!(
            v["Contents"],
            json!([
                { "Key": "a/b.txt", "Size": "10", "ETag": "\"x\"" },
                { "Key": "a/c.txt", "Size": "20", "ETag": "\"y\"" },
            ])
        );
    }

    #[test]
    fn empty_root_element_is_empty_object_shape() {
        let xml = r#"<ListBucketResult/>"#;
        let v = parse_xml_response(xml).unwrap();
        // No children at all — falls through to empty-string shape.
        assert_eq!(v, json!(""));
    }
}
