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
    /// Override for the AWS service endpoint URL. Defaults to
    /// `https://{service}.{region}.amazonaws.com/`. Set when pointing at
    /// DynamoDB Local, LocalStack, or a custom endpoint. Picked up from
    /// `AWS_ENDPOINT_URL` automatically by every constructor below.
    endpoint: Option<String>,
    /// Cap on how many pages auto-paginating list operations will fetch.
    /// `None` walks until AWS stops returning a continuation token.
    /// See [`AwsAccount::with_max_pages`].
    max_pages: Option<usize>,
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
                endpoint: env_endpoint(),
                max_pages: None,
                http: reqwest::Client::new(),
            }),
        }
    }

    /// Read `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, and
    /// `AWS_REGION` from the environment. Picks up `AWS_SESSION_TOKEN`
    /// for temporary credentials if present.
    pub fn from_env() -> Result<Self> {
        let access_key =
            std::env::var("AWS_ACCESS_KEY_ID").map_err(|_| error!("AWS_ACCESS_KEY_ID not set"))?;
        let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY")
            .map_err(|_| error!("AWS_SECRET_ACCESS_KEY not set"))?;
        let region = std::env::var("AWS_REGION").map_err(|_| error!("AWS_REGION not set"))?;
        let session_token = std::env::var("AWS_SESSION_TOKEN").ok();

        Ok(Self {
            inner: Arc::new(Inner {
                access_key,
                secret_key,
                session_token,
                region,
                endpoint: env_endpoint(),
                max_pages: None,
                http: reqwest::Client::new(),
            }),
        })
    }

    /// Read the profile named by `AWS_PROFILE` (or `default`) from
    /// `~/.aws/credentials`. For SSO / assume-role profiles whose
    /// credentials don't live in that file, falls back to shelling out
    /// to `aws configure export-credentials --profile <name> --format env`,
    /// which the AWS CLI uses as a public, stable handover format and
    /// which knows how to materialise SSO tokens, assumed-role chains, etc.
    ///
    /// Region resolution falls through `AWS_REGION` →
    /// `AWS_DEFAULT_REGION` → `~/.aws/config` profile `region`.
    pub fn from_credentials_file() -> Result<Self> {
        let profile = std::env::var("AWS_PROFILE").unwrap_or_else(|_| "default".to_string());
        Self::from_profile(&profile)
    }

    /// Build an `AwsAccount` from a specific profile name. See
    /// [`from_credentials_file`](Self::from_credentials_file) for the
    /// resolution algorithm.
    pub fn from_profile(profile: &str) -> Result<Self> {
        let home_dir = home_dir().ok_or_else(|| error!("HOME not set"))?;
        let region = resolve_region_for(&home_dir, profile)?;

        // 1. Static credentials in `~/.aws/credentials [profile]`.
        let creds_path = home_dir.join(".aws/credentials");
        if let Ok(creds_text) = std::fs::read_to_string(&creds_path)
            && let Some(creds) =
                parse_profile(&creds_text, profile, /* config_style = */ false)
            && let (Some(ak), Some(sk)) = (
                creds.get("aws_access_key_id"),
                creds.get("aws_secret_access_key"),
            )
        {
            return Ok(Self {
                inner: Arc::new(Inner {
                    access_key: ak.clone(),
                    secret_key: sk.clone(),
                    session_token: creds.get("aws_session_token").cloned(),
                    region,
                    endpoint: env_endpoint(),
                    max_pages: None,
                    http: reqwest::Client::new(),
                }),
            });
        }

        // 2. SSO, assume-role, or `credential_process` profile: shell
        //    out to the AWS CLI's canonical export. Requires
        //    `aws sso login` to have run recently for SSO profiles.
        let (ak, sk, token) = export_credentials_via_aws_cli(profile)?;
        Ok(Self {
            inner: Arc::new(Inner {
                access_key: ak,
                secret_key: sk,
                session_token: token,
                region,
                endpoint: env_endpoint(),
                max_pages: None,
                http: reqwest::Client::new(),
            }),
        })
    }

    /// Try [`from_env`](Self::from_env), fall back to
    /// [`from_credentials_file`](Self::from_credentials_file). Use
    /// this when you don't care which one — typical CLI / dev setup.
    pub fn from_default() -> Result<Self> {
        match Self::from_env() {
            Ok(acc) => Ok(acc),
            Err(_) => Self::from_credentials_file(),
        }
    }

    /// Return a copy with the region overridden. Useful when credentials
    /// come from `~/.aws/credentials` but the target region differs from
    /// the profile default (e.g. a test fixture provisioned in a fixed
    /// region regardless of the developer's local config).
    pub fn with_region(self, region: impl Into<String>) -> Self {
        let inner = &self.inner;
        Self {
            inner: std::sync::Arc::new(Inner {
                access_key: inner.access_key.clone(),
                secret_key: inner.secret_key.clone(),
                session_token: inner.session_token.clone(),
                region: region.into(),
                endpoint: inner.endpoint.clone(),
                max_pages: inner.max_pages,
                http: inner.http.clone(),
            }),
        }
    }

    /// Return a copy pointing at a custom service endpoint URL (e.g.
    /// `http://localhost:8000` for DynamoDB Local, or a LocalStack URL).
    /// SigV4 still applies — the host derived from the URL is folded
    /// into the canonical request, so the local server must accept the
    /// signature (DynamoDB Local does, with any access/secret values).
    pub fn with_endpoint(self, endpoint: impl Into<String>) -> Self {
        let inner = &self.inner;
        Self {
            inner: std::sync::Arc::new(Inner {
                access_key: inner.access_key.clone(),
                secret_key: inner.secret_key.clone(),
                session_token: inner.session_token.clone(),
                region: inner.region.clone(),
                endpoint: Some(endpoint.into()),
                max_pages: inner.max_pages,
                http: inner.http.clone(),
            }),
        }
    }

    /// Cap how many pages of results any single auto-paginating list
    /// operation will fetch from this account. `None` (the default)
    /// walks until AWS stops returning a continuation token. Pages
    /// past the cap are silently dropped — callers see a truncated
    /// result. Useful as a safety belt for accounts with many
    /// thousands of streams / functions / etc., or for content-bearing
    /// reads where "all of it" isn't what you want.
    ///
    /// Has no effect on operations that don't auto-paginate (only the
    /// CloudWatch Logs and ECS list endpoints do today).
    pub fn with_max_pages(self, n: usize) -> Self {
        let inner = &self.inner;
        Self {
            inner: std::sync::Arc::new(Inner {
                access_key: inner.access_key.clone(),
                secret_key: inner.secret_key.clone(),
                session_token: inner.session_token.clone(),
                region: inner.region.clone(),
                endpoint: inner.endpoint.clone(),
                max_pages: Some(n),
                http: inner.http.clone(),
            }),
        }
    }

    pub(crate) fn max_pages(&self) -> Option<usize> {
        self.inner.max_pages
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

    /// Resolve the endpoint URL and `Host` header for a request to
    /// `service`. Returns `(url, host)` — the URL ends in `/`, the host
    /// is what goes into the SigV4 canonical request and the wire
    /// `Host` header.
    pub(crate) fn endpoint_for(&self, service: &str) -> (String, String) {
        match self.inner.endpoint.as_deref() {
            Some(ep) => {
                let trimmed = ep.trim_end_matches('/');
                let host = trimmed
                    .split_once("://")
                    .map(|(_, rest)| rest)
                    .unwrap_or(trimmed)
                    .to_string();
                (format!("{trimmed}/"), host)
            }
            None => {
                let host = format!("{service}.{}.amazonaws.com", self.inner.region);
                (format!("https://{host}/"), host)
            }
        }
    }
}

impl std::fmt::Debug for AwsAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsAccount")
            .field("region", &self.inner.region)
            .field("endpoint", &self.inner.endpoint)
            .field("access_key", &"<redacted>")
            .field("secret_key", &"<redacted>")
            .field(
                "session_token",
                &self.inner.session_token.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Picks `AWS_ENDPOINT_URL` out of the environment so every constructor
/// honours it without forcing callers to chain `.with_endpoint(...)`. The
/// AWS CLI uses the same env var for the same purpose.
fn env_endpoint() -> Option<String> {
    std::env::var("AWS_ENDPOINT_URL")
        .ok()
        .filter(|s| !s.is_empty())
}

/// Pull a named profile's key=value pairs out of an AWS-style INI file.
///
/// `config_style: true` looks for `[profile <name>]` (the form used by
/// `~/.aws/config` for non-default profiles); `false` looks for `[<name>]`
/// (the form used by `~/.aws/credentials` and the bare `[default]`
/// section in `~/.aws/config`). The default profile uses `[default]` in
/// both files, so we always also accept it.
fn parse_profile(
    content: &str,
    profile: &str,
    config_style: bool,
) -> Option<HashMap<String, String>> {
    let target_section = if config_style && profile != "default" {
        format!("profile {}", profile)
    } else {
        profile.to_string()
    };

    let mut in_target = false;
    let mut found = false;
    let mut map = HashMap::new();

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_target = section.trim() == target_section;
            if in_target {
                found = true;
            }
            continue;
        }
        if in_target && let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }

    found.then_some(map)
}

/// Region resolution for a named profile.
/// Order: `AWS_REGION` env → `AWS_DEFAULT_REGION` env → profile entry in
/// `~/.aws/config`.
fn resolve_region_for(home_dir: &std::path::Path, profile: &str) -> Result<String> {
    if let Ok(r) = std::env::var("AWS_REGION") {
        return Ok(r);
    }
    if let Ok(r) = std::env::var("AWS_DEFAULT_REGION") {
        return Ok(r);
    }
    let config_path = home_dir.join(".aws/config");
    if let Ok(text) = std::fs::read_to_string(&config_path)
        && let Some(profile_map) = parse_profile(&text, profile, true)
        && let Some(r) = profile_map.get("region")
    {
        return Ok(r.clone());
    }
    Err(error!(
        "AWS region not found (set AWS_REGION, or add `region = ...` under the profile in ~/.aws/config)",
        profile = profile
    ))
}

/// Shell out to `aws configure export-credentials --profile X --format env`
/// to materialise creds for SSO, assume-role, and `credential_process`
/// profiles. The CLI prints `export AWS_ACCESS_KEY_ID=...` /
/// `export AWS_SECRET_ACCESS_KEY=...` / (optionally)
/// `export AWS_SESSION_TOKEN=...` lines to stdout. The session token is
/// absent for `credential_process` returning permanent IAM creds, so it's
/// optional in the return shape. Every failure path — CLI not installed,
/// CLI exit non-zero, output missing access/secret — surfaces as `Err`
/// with a specific message rather than collapsing into a generic
/// "profile not resolvable".
fn export_credentials_via_aws_cli(profile: &str) -> Result<(String, String, Option<String>)> {
    let output = match std::process::Command::new("aws")
        .args([
            "configure",
            "export-credentials",
            "--profile",
            profile,
            "--format",
            "env",
        ])
        .output()
    {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(error!(
                "AWS CLI not installed — needed to materialise SSO, assume-role, or credential_process credentials. Install via mise or your package manager.",
                profile = profile
            ));
        }
        Err(e) => return Err(error!(format!("failed to spawn `aws`: {e}"))),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(error!(
            "`aws configure export-credentials` failed — for SSO profiles try `aws sso login --profile <name>` first",
            profile = profile,
            stderr = stderr.trim().to_string()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut access_key = None;
    let mut secret_key = None;
    let mut session_token = None;
    for line in stdout.lines() {
        let line = line.trim();
        // The CLI uses `export KEY=VALUE`; tolerate `KEY=VALUE` too.
        let body = line.strip_prefix("export ").unwrap_or(line);
        if let Some((k, v)) = body.split_once('=') {
            match k.trim() {
                "AWS_ACCESS_KEY_ID" => access_key = Some(v.trim().to_string()),
                "AWS_SECRET_ACCESS_KEY" => secret_key = Some(v.trim().to_string()),
                "AWS_SESSION_TOKEN" => session_token = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }
    match (access_key, secret_key) {
        (Some(ak), Some(sk)) => Ok((ak, sk, session_token)),
        _ => Err(error!(
            "`aws configure export-credentials` returned no usable credentials (missing AWS_ACCESS_KEY_ID or AWS_SECRET_ACCESS_KEY)",
            profile = profile
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_profile;

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
        let p = parse_profile(ini, "default", false).expect("default section");
        assert_eq!(p.get("aws_access_key_id").unwrap(), "AKIA_DEFAULT");
        assert_eq!(p.get("aws_secret_access_key").unwrap(), "secret_default");
        assert_eq!(p.get("aws_session_token").unwrap(), "token_default");
    }

    #[test]
    fn picks_named_credentials_profile() {
        let ini = "\
[default]
aws_access_key_id = NOPE

[work]
aws_access_key_id = AKIA_WORK
aws_secret_access_key = secret_work
";
        let p = parse_profile(ini, "work", false).expect("work section");
        assert_eq!(p.get("aws_access_key_id").unwrap(), "AKIA_WORK");
    }

    #[test]
    fn picks_named_config_profile_uses_profile_prefix() {
        // ~/.aws/config writes named profiles as `[profile NAME]`,
        // not bare `[NAME]`.
        let ini = "\
[default]
region = eu-west-2

[profile work]
region = us-east-1
";
        let p = parse_profile(ini, "work", true).expect("work section");
        assert_eq!(p.get("region").unwrap(), "us-east-1");
        // And `default` in config still uses the bare form.
        let d = parse_profile(ini, "default", true).expect("default section");
        assert_eq!(d.get("region").unwrap(), "eu-west-2");
    }

    #[test]
    fn missing_profile_returns_none() {
        let ini = "[work]\naws_access_key_id = X\n";
        assert!(parse_profile(ini, "default", false).is_none());
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
        let p = parse_profile(ini, "default", false).unwrap();
        assert_eq!(p.get("aws_access_key_id").unwrap(), "AK");
        assert_eq!(p.get("aws_secret_access_key").unwrap(), "SK");
    }
}
