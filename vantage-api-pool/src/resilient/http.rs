//! The `reqwest::Client` behind a [`ResilientClient`](super::ResilientClient).
//!
//! A caller-supplied client is used as is. Otherwise the default client is
//! built on the first request, not at construction, because building it looks
//! up the system proxy configuration, which is slow on some platforms. The
//! first request pays that cost; pass a pre-built client to
//! `ResilientClientBuilder::http_client` to avoid it.

use std::time::Duration;

use tokio::sync::OnceCell;

/// How long one request may take, from sending to the last body byte, unless
/// the builder says otherwise.
pub(crate) const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct LazyHttp {
    cell: OnceCell<reqwest::Client>,
    timeout: Option<Duration>,
}

impl LazyHttp {
    /// Use `client` untouched: its own timeout and settings apply.
    pub(crate) fn supplied(client: reqwest::Client) -> Self {
        Self {
            cell: OnceCell::new_with(Some(client)),
            timeout: None,
        }
    }

    /// Build a default client on first use, with `timeout` as its request
    /// timeout (`None` for no timeout).
    pub(crate) fn deferred(timeout: Option<Duration>) -> Self {
        Self {
            cell: OnceCell::new(),
            timeout,
        }
    }

    /// The client, building it first if this is the first request.
    /// Concurrent first requests wait for one build. A failed build is not
    /// remembered: the next request tries again.
    pub(crate) async fn get(&self) -> Result<&reqwest::Client, reqwest::Error> {
        self.cell
            .get_or_try_init(|| async {
                let mut builder = reqwest::Client::builder();
                if let Some(timeout) = self.timeout {
                    builder = builder.timeout(timeout);
                }
                builder.build()
            })
            .await
    }
}
