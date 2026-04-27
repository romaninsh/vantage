//! `AwsAccount` — account-wide credentials handle.
//!
//! Holds the access key, secret key, and region. Cheap to clone (everything
//! lives behind an `Arc`). Used directly as the `TableSource` for JSON-1.1
//! tables — see `crate::json1` for the protocol impl. The per-operation
//! configuration (service, operation target, response array key) lives in
//! the table name, formatted as `array_key:service/target`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use vantage_core::Result;
use vantage_core::error;

#[derive(Clone)]
pub struct AwsAccount {
    inner: Arc<Inner>,
}

struct Inner {
    access_key: String,
    secret_key: String,
    session_token: Option<String>,
    region: String,
    http: reqwest::Client,
}

impl AwsAccount {
    /// Construct from explicit static credentials.
    pub fn new(
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        region: impl Into<String>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                access_key: access_key.into(),
                secret_key: secret_key.into(),
                session_token: None,
                region: region.into(),
                http: reqwest::Client::new(),
            }),
        }
    }

    /// Construct from `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`,
    /// optional `AWS_SESSION_TOKEN`, and `AWS_REGION` environment
    /// variables. The credential chain (profiles, IMDS, SSO) is out of
    /// scope for v0 — set the env vars yourself or call `new`.
    pub fn from_env() -> Result<Self> {
        let access_key = std::env::var("AWS_ACCESS_KEY_ID")
            .map_err(|_| error!("AWS_ACCESS_KEY_ID not set"))?;
        let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY")
            .map_err(|_| error!("AWS_SECRET_ACCESS_KEY not set"))?;
        let region = std::env::var("AWS_REGION")
            .map_err(|_| error!("AWS_REGION not set"))?;
        let session_token = std::env::var("AWS_SESSION_TOKEN").ok();

        Ok(Self {
            inner: Arc::new(Inner {
                access_key,
                secret_key,
                session_token,
                region,
                http: reqwest::Client::new(),
            }),
        })
    }

    /// Construct from `~/.aws/credentials` and `~/.aws/config`, reading
    /// only the `[default]` profile. Named profiles, `AWS_PROFILE`,
    /// SSO, and the assume-role flow are out of scope for v0.
    ///
    /// Region resolution: `AWS_REGION` env var first, then
    /// `AWS_DEFAULT_REGION`, then the `region` key in `~/.aws/config`'s
    /// `[default]` profile.
    pub fn from_credentials_file() -> Result<Self> {
        let home_dir = home_dir().ok_or_else(|| error!("HOME not set"))?;
        let creds_path = home_dir.join(".aws/credentials");
        let creds_text = std::fs::read_to_string(&creds_path)
            .map_err(|e| error!(format!("failed to read {}: {}", creds_path.display(), e)))?;
        let creds = parse_default_profile(&creds_text).ok_or_else(|| {
            error!(format!("no [default] profile in {}", creds_path.display()))
        })?;

        let access_key = creds
            .get("aws_access_key_id")
            .ok_or_else(|| {
                error!(format!(
                    "aws_access_key_id missing in {} [default]",
                    creds_path.display()
                ))
            })?
            .clone();
        let secret_key = creds
            .get("aws_secret_access_key")
            .ok_or_else(|| {
                error!(format!(
                    "aws_secret_access_key missing in {} [default]",
                    creds_path.display()
                ))
            })?
            .clone();
        let session_token = creds.get("aws_session_token").cloned();

        let region = std::env::var("AWS_REGION")
            .ok()
            .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok())
            .or_else(|| {
                let config_path = home_dir.join(".aws/config");
                let text = std::fs::read_to_string(&config_path).ok()?;
                parse_default_profile(&text)?.get("region").cloned()
            })
            .ok_or_else(|| {
                error!(
                    "AWS region not found (set AWS_REGION or add region to ~/.aws/config [default])"
                )
            })?;

        Ok(Self {
            inner: Arc::new(Inner {
                access_key,
                secret_key,
                session_token,
                region,
                http: reqwest::Client::new(),
            }),
        })
    }

    /// Try [`from_env`](Self::from_env) first, then fall back to
    /// [`from_credentials_file`](Self::from_credentials_file). The
    /// error returned on failure is the file-based one — env-var
    /// failures are silently swallowed since they're the expected
    /// "not configured this way" path.
    pub fn from_default() -> Result<Self> {
        match Self::from_env() {
            Ok(acc) => Ok(acc),
            Err(_) => Self::from_credentials_file(),
        }
    }

    pub(crate) fn region(&self) -> &str {
        &self.inner.region
    }

    pub(crate) fn access_key(&self) -> &str {
        &self.inner.access_key
    }

    pub(crate) fn secret_key(&self) -> &str {
        &self.inner.secret_key
    }

    pub(crate) fn session_token(&self) -> Option<&str> {
        self.inner.session_token.as_deref()
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.inner.http
    }
}

impl std::fmt::Debug for AwsAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsAccount")
            .field("region", &self.inner.region)
            .field("access_key", &"<redacted>")
            .field("secret_key", &"<redacted>")
            .field("session_token", &self.inner.session_token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Pull the `[default]` section from an AWS-style INI file. Returns
/// `None` if no `[default]` section was seen. `~/.aws/config` happens
/// to use `[default]` (not `[profile default]`) for the default
/// profile, so the same parser handles both files.
fn parse_default_profile(content: &str) -> Option<HashMap<String, String>> {
    let mut in_default = false;
    let mut found_default = false;
    let mut map = HashMap::new();

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_default = section.trim() == "default";
            if in_default {
                found_default = true;
            }
            continue;
        }
        if in_default {
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }

    found_default.then_some(map)
}

#[cfg(test)]
mod tests {
    use super::parse_default_profile;

    #[test]
    fn picks_default_section_only() {
        let ini = "\
[other]
aws_access_key_id = NOPE
aws_secret_access_key = NOPE

[default]
aws_access_key_id = AKIA_DEFAULT
aws_secret_access_key = secret_default
aws_session_token = token_default

[another]
aws_access_key_id = ALSO_NOPE
";
        let p = parse_default_profile(ini).expect("default section");
        assert_eq!(p.get("aws_access_key_id").unwrap(), "AKIA_DEFAULT");
        assert_eq!(p.get("aws_secret_access_key").unwrap(), "secret_default");
        assert_eq!(p.get("aws_session_token").unwrap(), "token_default");
    }

    #[test]
    fn no_default_returns_none() {
        let ini = "[work]\naws_access_key_id = X\n";
        assert!(parse_default_profile(ini).is_none());
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let ini = "\
# top comment
; also a comment

[default]
# inline comment line
aws_access_key_id = AK
  aws_secret_access_key  =  SK
";
        let p = parse_default_profile(ini).unwrap();
        assert_eq!(p.get("aws_access_key_id").unwrap(), "AK");
        assert_eq!(p.get("aws_secret_access_key").unwrap(), "SK");
    }
}
