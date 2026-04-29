//! AWS [`Arn`] — parsed Amazon Resource Name.
//!
//! Wire format: `arn:partition:service:region:account-id:resource`. The
//! `region` and `account-id` segments are commonly empty (global
//! services like IAM have no region; STS responses sometimes elide
//! account). The `resource` segment may itself contain colons or
//! slashes — we keep it as a single string and don't try to parse
//! `resource-type/resource-id` further (callers can split if needed).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

use vantage_types::{RichText, Style, TerminalRender};

/// Parsed AWS ARN.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Arn {
    pub partition: String,
    pub service: String,
    pub region: String,
    pub account_id: String,
    pub resource: String,
}

impl Arn {
    /// Parse an ARN. Panics on malformed input — this is a hard
    /// invariant violation: AWS only ever returns well-formed ARNs, so
    /// a panic here means something upstream broke.
    ///
    /// Use [`Arn::try_parse`] when the input might genuinely be a
    /// non-ARN string.
    pub fn parse(s: &str) -> Self {
        Self::try_parse(s).unwrap_or_else(|e| panic!("invalid ARN {s:?}: {e}"))
    }

    /// Non-panicking parser. Returns `Err` with a short reason on
    /// malformed input.
    pub fn try_parse(s: &str) -> Result<Self, &'static str> {
        // ARN has exactly 6 colon-separated parts: arn:partition:service:region:account:resource
        // The resource part may itself contain colons, so split with limit.
        let mut parts = s.splitn(6, ':');
        let prefix = parts.next().ok_or("empty input")?;
        if prefix != "arn" {
            return Err("must start with \"arn:\"");
        }
        let partition = parts.next().ok_or("missing partition")?;
        let service = parts.next().ok_or("missing service")?;
        let region = parts.next().ok_or("missing region")?;
        let account_id = parts.next().ok_or("missing account-id")?;
        let resource = parts.next().ok_or("missing resource")?;

        if partition.is_empty() {
            return Err("partition must not be empty");
        }
        if service.is_empty() {
            return Err("service must not be empty");
        }
        if resource.is_empty() {
            return Err("resource must not be empty");
        }

        Ok(Arn {
            partition: partition.to_string(),
            service: service.to_string(),
            region: region.to_string(),
            account_id: account_id.to_string(),
            resource: resource.to_string(),
        })
    }
}

impl fmt::Display for Arn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "arn:{}:{}:{}:{}:{}",
            self.partition, self.service, self.region, self.account_id, self.resource
        )
    }
}

impl FromStr for Arn {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Arn::try_parse(s)
    }
}

impl Serialize for Arn {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Arn {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        Arn::try_parse(&s).map_err(serde::de::Error::custom)
    }
}

impl TerminalRender for Arn {
    fn render(&self) -> RichText {
        // arn : partition : service : region : account : resource
        // dim   dim         info      dim      muted     default
        let mut rt = RichText::new()
            .push("arn", Style::Dim)
            .push(":", Style::Muted)
            .push(&self.partition, Style::Dim)
            .push(":", Style::Muted)
            .push(&self.service, Style::Info)
            .push(":", Style::Muted);

        if self.region.is_empty() {
            rt = rt.push(":", Style::Muted);
        } else {
            rt = rt.push(&self.region, Style::Dim).push(":", Style::Muted);
        }

        if self.account_id.is_empty() {
            rt = rt.push(":", Style::Muted);
        } else {
            rt = rt
                .push(&self.account_id, Style::Muted)
                .push(":", Style::Muted);
        }

        rt.push(&self.resource, Style::Default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iam_user_arn() {
        let a = Arn::parse("arn:aws:iam::123456789012:user/alice");
        assert_eq!(a.partition, "aws");
        assert_eq!(a.service, "iam");
        assert_eq!(a.region, "");
        assert_eq!(a.account_id, "123456789012");
        assert_eq!(a.resource, "user/alice");
    }

    #[test]
    fn parses_resource_with_colons() {
        // Lambda layer ARNs have version after a colon in the resource.
        let a = Arn::parse("arn:aws:lambda:us-east-1:123456789012:layer:my-layer:3");
        assert_eq!(a.service, "lambda");
        assert_eq!(a.region, "us-east-1");
        assert_eq!(a.resource, "layer:my-layer:3");
    }

    #[test]
    fn round_trips_through_display() {
        let s = "arn:aws:s3:::my-bucket/key/with/slashes";
        let a = Arn::parse(s);
        assert_eq!(a.to_string(), s);
    }

    #[test]
    #[should_panic(expected = "invalid ARN")]
    fn panics_on_malformed_input() {
        let _ = Arn::parse("not-an-arn");
    }

    #[test]
    fn try_parse_returns_err_for_garbage() {
        assert!(Arn::try_parse("").is_err());
        assert!(Arn::try_parse("foo:bar").is_err());
        assert!(Arn::try_parse("arn:aws:iam::123:").is_err()); // empty resource
    }

    #[test]
    fn renders_with_styled_segments() {
        let a = Arn::parse("arn:aws:iam::123:user/alice");
        let rt = a.render();
        // Should produce more than one span.
        assert!(rt.spans.len() > 3);
        // Concatenated text round-trips.
        assert_eq!(rt.to_plain(), "arn:aws:iam::123:user/alice");
    }
}
