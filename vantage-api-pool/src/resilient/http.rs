//! The `reqwest::Client` behind a [`ResilientClient`](super::ResilientClient).
//!
//! A caller-supplied client is used as is. Otherwise the default client is
//! built on the first request, not at construction: building one looks up the
//! system proxy configuration, which is slow on some platforms and wasted work
//! for a client that is constructed but never used.
//!
//! The first request builds the client on the calling task, system proxy
//! lookup included, so that request pays the cost. Pass a pre-built client
//! to `ResilientClientBuilder::http_client` to avoid it.

use std::time::Duration;

use tokio::sync::OnceCell;

/// How long one request may take, from sending to the last body byte, unless
/// the builder says otherwise.
pub(crate) const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct LazyHttp {
    cell: OnceCell<reqwest::Client>,
    timeout: Option<Duration>,
    #[cfg(test)]
    builds: std::sync::atomic::AtomicUsize,
}

impl LazyHttp {
    /// Use `client` untouched: its own timeout and settings apply.
    pub(crate) fn supplied(client: reqwest::Client) -> Self {
        Self {
            cell: OnceCell::new_with(Some(client)),
            timeout: None,
            #[cfg(test)]
            builds: Default::default(),
        }
    }

    /// Build a default client on first use, with `timeout` as its request
    /// timeout (`None` for no timeout).
    pub(crate) fn deferred(timeout: Option<Duration>) -> Self {
        Self {
            cell: OnceCell::new(),
            timeout,
            #[cfg(test)]
            builds: Default::default(),
        }
    }

    /// The client, building it first if this is the first request.
    /// Concurrent first requests wait for one build. A failed build is not
    /// remembered: the next request tries again.
    pub(crate) async fn get(&self) -> Result<&reqwest::Client, reqwest::Error> {
        self.cell
            .get_or_try_init(|| async {
                #[cfg(test)]
                self.builds
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut builder = reqwest::Client::builder();
                if let Some(timeout) = self.timeout {
                    builder = builder.timeout(timeout);
                }
                builder.build()
            })
            .await
    }

    #[cfg(test)]
    pub(crate) fn is_built(&self) -> bool {
        self.cell.initialized()
    }

    #[cfg(test)]
    pub(crate) fn build_count(&self) -> usize {
        self.builds.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use crate::ResilientClient;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn default_client_is_built_on_first_request_not_at_construction() {
        let client = ResilientClient::builder().build();
        assert!(!client.http.is_built(), "construction must not build it");

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let url = server.uri();
        client.execute(|h| h.get(&url)).await.expect("2xx");

        assert!(client.http.is_built(), "the first request builds it");
    }

    #[tokio::test]
    async fn concurrent_first_requests_build_the_client_once() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let client = ResilientClient::builder().build();
        let url = server.uri();

        let calls = (0..8).map(|_| client.execute(|h| h.get(&url)));
        for result in futures_util::future::join_all(calls).await {
            result.expect("2xx");
        }

        assert_eq!(client.http.build_count(), 1);
    }

    #[tokio::test]
    async fn supplied_client_is_used_as_is() {
        let client = ResilientClient::builder()
            .http_client(reqwest::Client::new())
            .build();
        assert!(client.http.is_built());
    }
}
