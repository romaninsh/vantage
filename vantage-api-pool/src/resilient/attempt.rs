//! One `execute_with` call, as a loop over three steps: `wait_for_gate` (circuit
//! breaker, then rate-limit token), `send_once` (the request), `record`
//! (breaker, observer, what to do next).
//!
//! The parallelism permit is held only inside `send_once`, so nothing that
//! sleeps for a policy reason (cooldown, token wait, back-off) occupies the
//! client's parallelism budget. A `FailFast` caller is never queued behind a
//! caller waiting out a cooldown.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::breaker::{Gate, ProbeGuard};
use super::policy::{self, Health, with_jitter};
use super::{BreakerMode, CallPolicy, ClientError, ErrorKind, ResilientClient, TransportEvent};

/// How much of a failing response body is kept on [`ClientError::body`].
const BODY_CAP: usize = 2048;

enum Outcome {
    Success {
        resp: reqwest::Response,
        ms: u64,
    },
    /// A `401` before this call's token was re-acquired.
    Unauthorized {
        ms: u64,
    },
    Failed {
        kind: ErrorKind,
        body: Option<String>,
        /// The server's `Retry-After`, when it sent a usable one.
        hint: Option<Duration>,
        ms: u64,
    },
}

enum Decision {
    Return(Result<reqwest::Response, ClientError>),
    Retry(Duration),
    /// Attempt again at once with the fresh token. Spends no retry budget and
    /// reports no `RetryScheduled`.
    Replay,
}

pub(super) struct AttemptLoop<'a, F> {
    client: &'a ResilientClient,
    policy: &'a CallPolicy,
    build: &'a F,
    /// Retries made so far.
    attempt: usize,
    /// The token was already re-acquired for this call, so another `401` is a
    /// real authorization failure.
    refreshed: bool,
    /// Set between `Started` and the terminal event: dropping the future in
    /// that window reports `Cancelled`.
    pending: Option<PendingAttempt<'a>>,
}

struct PendingAttempt<'a> {
    client: &'a ResilientClient,
    armed: bool,
}

impl PendingAttempt<'_> {
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for PendingAttempt<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.client.report(TransportEvent::Cancelled);
        }
    }
}

impl<'a, F> AttemptLoop<'a, F>
where
    F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
{
    pub(super) async fn run(
        client: &'a ResilientClient,
        policy: &'a CallPolicy,
        build: &'a F,
    ) -> Result<reqwest::Response, ClientError> {
        let mut call = Self {
            client,
            policy,
            build,
            attempt: 0,
            refreshed: false,
            pending: None,
        };
        loop {
            let probe = call.wait_for_gate().await?;
            let outcome = call.send_once().await?;
            match call.record(outcome, probe).await {
                Decision::Return(result) => return result,
                Decision::Replay => {}
                Decision::Retry(after) => tokio::time::sleep(after).await,
            }
        }
    }

    /// Wait for the breaker, then a rate-limit token, holding no permit.
    /// Returns the probe guard when this attempt was granted the probe slot.
    async fn wait_for_gate(&self) -> Result<Option<ProbeGuard>, ClientError> {
        let mut probe = None;
        if let Some(b) = &self.client.breaker {
            loop {
                match b.gate() {
                    Gate::Allow { probe: epoch } => {
                        probe = epoch.map(|e| ProbeGuard::new(Arc::clone(b), e));
                        break;
                    }
                    Gate::OpenFor(wait) => match self.policy.breaker {
                        BreakerMode::FailFast => {
                            return Err(self.report_rejection(
                                ErrorKind::BreakerOpen,
                                self.attempt,
                                0,
                            ));
                        }
                        BreakerMode::WaitForProbe => tokio::time::sleep(wait).await,
                    },
                }
            }
        }
        if let Some(r) = &self.client.rate {
            r.acquire().await;
        }
        Ok(probe)
    }

    /// Build, authenticate and send one request, holding the permit and the
    /// in-flight count only for this window.
    async fn send_once(&mut self) -> Result<Outcome, ClientError> {
        let _permit = self.client.semaphore.acquire().await.map_err(|_| {
            ClientError::new(
                ErrorKind::Transport("client semaphore closed".into()),
                self.attempt,
            )
        })?;
        let _in_flight = self.client.enter_flight();

        self.client.report(TransportEvent::Started);
        self.pending = Some(PendingAttempt {
            client: self.client,
            armed: true,
        });

        let http = match self.client.http.get().await {
            Ok(http) => http,
            Err(e) => {
                return Ok(Outcome::Failed {
                    kind: ErrorKind::Transport(format!("building the HTTP client failed: {e}")),
                    body: None,
                    hint: None,
                    ms: 0,
                });
            }
        };
        let mut req = (self.build)(http);
        if let Some(auth) = &self.client.auth {
            let acquiring = Instant::now();
            match auth.current().await {
                Ok(token) => req = req.header(&auth.header, format!("{}{token}", auth.scheme)),
                Err(e) => {
                    return Ok(Outcome::Failed {
                        kind: ErrorKind::Auth(e.to_string()),
                        body: None,
                        hint: None,
                        ms: elapsed_ms(acquiring),
                    });
                }
            }
        }

        // `ms` starts here: the auth round trip is the client's own cost, not
        // the API's latency.
        let sending = Instant::now();
        let sent = req.send().await;
        let ms = elapsed_ms(sending);

        Ok(match sent {
            Ok(resp) if resp.status().is_success() => Outcome::Success { resp, ms },
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status == 401 && !self.refreshed && self.client.auth.is_some() {
                    return Ok(Outcome::Unauthorized { ms });
                }
                let hint = retry_after(&resp);
                Outcome::Failed {
                    kind: ErrorKind::Status(status),
                    body: capture_body(resp).await,
                    hint,
                    ms,
                }
            }
            Err(e) => Outcome::Failed {
                kind: ErrorKind::Transport(e.to_string()),
                body: None,
                hint: None,
                ms,
            },
        })
    }

    /// Apply the outcome to the breaker, tell the observer, and decide what
    /// the loop does next. Consumes the probe guard, releasing the slot before
    /// any observer callback or sleep.
    async fn record(&mut self, outcome: Outcome, probe: Option<ProbeGuard>) -> Decision {
        match outcome {
            Outcome::Success { resp, ms } => {
                let succeeded = TransportEvent::Succeeded {
                    status: resp.status().as_u16(),
                    ms,
                    bytes: resp.content_length(),
                };
                self.settle_and_report(probe, Health::Healthy, succeeded);
                Decision::Return(Ok(resp))
            }

            Outcome::Unauthorized { ms } => {
                let failed = TransportEvent::Failed {
                    error: ClientError::new(ErrorKind::Status(401), self.attempt + 1),
                    ms,
                };
                self.settle_and_report(probe, Health::Reachable, failed);

                self.refreshed = true;
                let auth = self
                    .client
                    .auth
                    .as_ref()
                    .expect("Unauthorized is only produced when a refresher is configured");
                let acquiring = Instant::now();
                match auth.reacquire().await {
                    Ok(_) => Decision::Replay,
                    Err(e) => Decision::Return(Err(self.report_rejection(
                        ErrorKind::Auth(e.to_string()),
                        self.attempt + 1,
                        elapsed_ms(acquiring),
                    ))),
                }
            }

            Outcome::Failed {
                kind,
                body,
                hint,
                ms,
            } => {
                let (retryable, health) = match &kind {
                    ErrorKind::Status(s) => {
                        (policy::is_retryable_status(*s), policy::status_health(*s))
                    }
                    ErrorKind::Transport(_) => (true, Health::Failing),
                    // A failed auth refresh says nothing about the API.
                    _ => (false, Health::Inconclusive),
                };
                let error = ClientError::new(kind, self.attempt + 1).with_body(body);
                let failed = TransportEvent::Failed {
                    error: error.clone(),
                    ms,
                };
                self.settle_and_report(probe, health, failed);

                if !retryable {
                    return Decision::Return(Err(error));
                }
                let Some(delay) = self.policy.next_backoff(self.attempt) else {
                    return Decision::Return(Err(error));
                };
                self.attempt += 1;
                let cap = self.policy.retry_after_cap().unwrap_or(delay);
                let after = with_jitter(hint.map_or(delay, |h| h.min(cap)));
                self.client.report(TransportEvent::RetryScheduled {
                    after,
                    attempt: self.attempt,
                });
                Decision::Retry(after)
            }
        }
    }

    /// Tell the breaker what the attempt proved, then report its terminal
    /// event, then the breaker transition it caused: the order the observer
    /// contract promises.
    fn settle_and_report(
        &mut self,
        probe: Option<ProbeGuard>,
        health: Health,
        terminal: TransportEvent,
    ) {
        if let Some(pending) = self.pending.take() {
            pending.disarm();
        }
        let transition = self.settle(probe, health);
        self.client.report(terminal);
        if let Some(event) = transition {
            self.client.report(event);
        }
    }

    /// Returns the breaker transition to report, if the state changed.
    fn settle(&self, probe: Option<ProbeGuard>, health: Health) -> Option<TransportEvent> {
        let event = self.client.breaker.as_ref().and_then(|b| match health {
            Health::Healthy => b.record_success().then_some(TransportEvent::BreakerClosed),
            Health::Reachable => b
                .record_reachable()
                .then_some(TransportEvent::BreakerClosed),
            Health::Inconclusive => None,
            Health::Failing => b
                .record_failure(probe.as_ref().map(ProbeGuard::epoch))
                .map(|cooldown| TransportEvent::BreakerOpened { cooldown }),
        });
        drop(probe);
        event
    }

    /// Report an attempt that failed with no request sent, as a paired
    /// `Started` / `Failed`.
    fn report_rejection(&self, kind: ErrorKind, attempts: usize, ms: u64) -> ClientError {
        self.client.report(TransportEvent::Started);
        let error = ClientError::new(kind, attempts);
        self.client.report(TransportEvent::Failed {
            error: error.clone(),
            ms,
        });
        error
    }
}

fn elapsed_ms(since: Instant) -> u64 {
    since.elapsed().as_millis() as u64
}

/// The server's `Retry-After`, in whole seconds. Sub-second and HTTP-date
/// forms are ignored: the policy's own back-off is a better guess than either.
fn retry_after(resp: &reqwest::Response) -> Option<Duration> {
    let secs: u64 = resp
        .headers()
        .get("retry-after")?
        .to_str()
        .ok()?
        .parse()
        .ok()?;
    (secs >= 1).then(|| Duration::from_secs(secs))
}

/// The start of a failing response body, for [`ClientError::body`]. A body
/// that cannot be read at all is simply absent — the status is the failure,
/// this is only context.
async fn capture_body(resp: reqwest::Response) -> Option<String> {
    let mut text = resp.text().await.ok()?;
    if text.len() > BODY_CAP {
        let mut end = BODY_CAP;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    (!text.is_empty()).then_some(text)
}
