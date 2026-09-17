//! One `execute_with` call, as a loop over three named steps.
//!
//! Per attempt, in this order: `wait_for_gate` waits out the circuit breaker
//! and then takes a rate-limit token, holding no permit; `send_once` acquires
//! a permit and covers only build, send and the response head; `record`
//! applies the outcome to the breaker, tells the observer and says what the
//! loop does next. The permit is dropped when `send_once` returns, so nothing
//! that sleeps for a policy reason — a cooldown, a token wait, a retry
//! back-off — occupies the client's parallelism budget. A `FailFast` caller
//! is therefore never queued behind a caller waiting out a cooldown.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::breaker::{Gate, ProbeGuard};
use super::policy::{self, with_jitter, Health};
use super::{BreakerMode, CallPolicy, ClientError, ErrorKind, ResilientClient, TransportEvent};

/// How much of a failing response body is kept on [`ClientError::body`].
const BODY_CAP: usize = 2048;

/// What one attempt produced, before the breaker and the retry policy see it.
enum Outcome {
    Success {
        resp: reqwest::Response,
        ms: u64,
    },
    /// A `401` on a client whose token has not yet been re-acquired for this
    /// call: the token is refreshed and the attempt replayed.
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

/// What the loop does once an outcome has been recorded.
enum Decision {
    Return(Result<reqwest::Response, ClientError>),
    /// Sleep this long, then attempt again. The attempt counter has advanced
    /// and `RetryScheduled` has been reported.
    Retry(Duration),
    /// Attempt again at once with the freshly acquired token. Not a retry: it
    /// spends no retry budget and reports no `RetryScheduled`.
    Replay,
}

pub(super) struct AttemptLoop<'a, F> {
    client: &'a ResilientClient,
    policy: &'a CallPolicy,
    build: &'a F,
    /// Retries made so far; the attempt now running is number `attempt + 1`.
    attempt: usize,
    /// The token has already been re-acquired once for this call, so another
    /// `401` is a genuine authorization failure and not a stale token.
    refreshed: bool,
    /// Set between `Started` and the attempt's terminal event. Dropping the
    /// call's future in that window drops the guard, which reports
    /// `Cancelled` so the observer never sees a `Started` without an end.
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
            // Held only for this attempt. Every early return below drops it,
            // and so does the enclosing future being cancelled, so a probe
            // that never records an outcome cannot jam the breaker open.
            let probe = call.wait_for_gate().await?;
            let outcome = call.send_once().await?;
            match call.record(outcome, probe).await {
                Decision::Return(result) => return result,
                Decision::Replay => {}
                Decision::Retry(after) => tokio::time::sleep(after).await,
            }
        }
    }

    /// Wait until this attempt may run: first the circuit breaker, then a
    /// rate-limit token. Neither wait holds a semaphore permit. Returns the
    /// guard for the half-open probe slot when this attempt was granted it.
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
                            ))
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

    /// Build, authenticate and send one request. The permit and the in-flight
    /// count are held for exactly this window and released before the caller
    /// sleeps for anything.
    async fn send_once(&mut self) -> Result<Outcome, ClientError> {
        let _permit = self
            .client
            .semaphore
            .acquire()
            .await
            .map_err(|_| ClientError::new(ErrorKind::Closed, self.attempt))?;
        let _in_flight = self.client.enter_flight();

        self.client.report(TransportEvent::Started);
        self.pending = Some(PendingAttempt {
            client: self.client,
            armed: true,
        });

        let mut req = (self.build)(&self.client.http);
        if let Some(auth) = &self.client.auth {
            let acquiring = Instant::now();
            match auth.current().await {
                Ok(token) => req = req.header(&auth.header, format!("{}{token}", auth.scheme)),
                // Reported as this attempt's terminal event by `record`, so
                // the `Started` above is not left dangling.
                Err(e) => {
                    return Ok(Outcome::Failed {
                        kind: ErrorKind::Auth(e.to_string()),
                        body: None,
                        hint: None,
                        ms: elapsed_ms(acquiring),
                    })
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
    /// the loop does next. The probe guard is consumed here: the slot is
    /// released as soon as the breaker has been told, before any observer
    /// callback runs and before any sleep.
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
                // A 401 is an answer: it closes an open breaker, but it is no
                // evidence of health, so the failure run stands.
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
                    // An auth refresher that failed says nothing about the
                    // API, and no retry will fix it.
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
                // A server asking for two minutes must not outlast the
                // caller's own ceiling.
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

    /// Settle one attempt: tell the breaker what it proved, then report its
    /// terminal event, then the breaker transition it caused. That order is
    /// what the observer contract promises — the probe slot is free before any
    /// callback runs, and a transition follows the attempt that caused it.
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

    /// Tell the breaker what this attempt proved, then release the probe
    /// slot. Returns the transition to report, if the breaker changed state.
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

    /// Report an attempt that failed without a request going out — a fail-fast
    /// breaker rejection, or a token refresh that never produced one. The
    /// observer still sees a paired `Started` / `Failed`.
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
