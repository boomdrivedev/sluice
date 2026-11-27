//! How sluice reaches GitHub's API: octocrab, over a tower stack of its
//! own on hyper's client
//! (`sluice-host-reads-github-through-octocrab-with-tower-layers@57c2759bda97`).
//!
//! The stack has three layers, from the outside in:
//!
//! - tower's `GlobalConcurrencyLimitLayer`, on one semaphore for the
//!   process, [`GITHUB`]: at most [`AT_ONCE`] calls are in flight at one
//!   time, whichever app, binding or thread sends them. `sluice host` is
//!   the one GitHub client on a host
//!   (`one-github-client-per-host@786dccccffe1`), so its limit is the
//!   host's.
//! - tower's `TimeoutLayer`: no try waits longer for its answer than any
//!   try of a call to a remote may, [`crate::retry::TRY_FOR`].
//! - [`BudgetLayer`], sluice's own: it reads what GitHub says of its
//!   allowance with each answer into a [`Budget`] on the answer, which
//!   the app keeps as a pause where it says the allowance is spent (134;
//!   rule core.29).
//!
//! Under them, hyper's client speaks HTTP/1 over rustls on ring with the
//! webpki roots, and follows no redirect: GitHub answers a renamed
//! repository's old address with one, and a call followed there would go
//! without its token. Each try runs under sluice's one retry policy, which
//! the app holds around the call (rule core.29): it reads answers the
//! stack does not, a token GitHub refused among them.
//!
//! The app is synchronous, so each [`Client`] holds a current-thread
//! runtime that it enters for each call alone, as the calls to AWS do.

use std::sync::{Arc, LazyLock};
use std::task::{Context, Poll};

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use http::{HeaderMap, Request, Response, Uri};
use http_body_util::BodyExt;
use hyper_util::client::legacy::connect::HttpConnector;
use octocrab::{AuthState, Octocrab, OctocrabBuilder};
use tokio::sync::Semaphore;
use tower::limit::GlobalConcurrencyLimitLayer;
use tower::timeout::TimeoutLayer;
use tower::{Layer, Service};

use crate::retry::{Failed, Judged, TRY_FOR};

/// How many calls to GitHub the process has in flight at one time at
/// most. GitHub takes 100 at most from one client, and asks a client to
/// send its calls one after another where it can: a few at a time keep
/// one repository's slow call from holding every other repository's.
pub const AT_ONCE: usize = 4;

/// The process's one limit on its calls to GitHub in flight, which every
/// [`Client`] takes a place under ([`AT_ONCE`]).
pub static GITHUB: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(AT_ONCE)));

/// What GitHub said of its allowance with an answer: the calls it leaves,
/// when it is whole again, which allowance it is (`core`, `graphql`), and
/// how many seconds to wait where it names them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    pub remaining: Option<u64>,
    pub reset: Option<DateTime<Utc>>,
    pub resource: Option<String>,
    pub retry_after: Option<u64>,
}

impl Budget {
    /// The budget `headers` give.
    pub fn read(headers: &HeaderMap) -> Budget {
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let number = |name: &str| header(name).and_then(|v| v.trim().parse::<u64>().ok());
        Budget {
            remaining: number("x-ratelimit-remaining"),
            reset: number("x-ratelimit-reset").and_then(|t| DateTime::from_timestamp(t as i64, 0)),
            resource: header("x-ratelimit-resource"),
            retry_after: number("retry-after"),
        }
    }
}

/// The layer that reads GitHub's [`Budget`] from each answer's headers
/// onto the answer.
#[derive(Clone, Copy, Debug, Default)]
pub struct BudgetLayer;

impl<S> Layer<S> for BudgetLayer {
    type Service = BudgetService<S>;

    fn layer(&self, inner: S) -> BudgetService<S> {
        BudgetService { inner }
    }
}

/// [`BudgetLayer`]'s service.
#[derive(Clone, Debug)]
pub struct BudgetService<S> {
    inner: S,
}

impl<S, Q, B> Service<Request<Q>> for BudgetService<S>
where
    S: Service<Request<Q>, Response = Response<B>>,
{
    type Response = Response<B>;
    type Error = S::Error;
    type Future = futures_util::future::MapOk<S::Future, fn(Response<B>) -> Response<B>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Q>) -> Self::Future {
        use futures_util::TryFutureExt;
        self.inner
            .call(req)
            .map_ok(budgeted::<B> as fn(Response<B>) -> Response<B>)
    }
}

/// `answer`, carrying the budget its headers give.
fn budgeted<B>(mut answer: Response<B>) -> Response<B> {
    let budget = Budget::read(answer.headers());
    answer.extensions_mut().insert(budget);
    answer
}

/// Where a call's connection goes in place of the address it names: a
/// test's, which can send a call where no connection is ever made.
pub type Reroute = Arc<dyn Fn(Uri) -> Uri + Send + Sync>;

/// hyper's connector, each address given to `reroute` first where one is
/// set.
#[derive(Clone)]
struct Connector {
    http: HttpConnector,
    reroute: Option<Reroute>,
}

impl Service<Uri> for Connector {
    type Response = <HttpConnector as Service<Uri>>::Response;
    type Error = <HttpConnector as Service<Uri>>::Error;
    type Future = <HttpConnector as Service<Uri>>::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.http.poll_ready(cx)
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        let uri = match &self.reroute {
            Some(r) => r(uri),
            None => uri,
        };
        self.http.call(uri)
    }
}

/// One call's answer as it came: its status, its headers, its budget and
/// its body's text.
pub struct Raw {
    pub status: u16,
    pub headers: HeaderMap,
    pub budget: Budget,
    pub text: String,
}

/// A client of GitHub's API at one address, on its own runtime.
pub struct Client {
    runtime: tokio::runtime::Runtime,
    octocrab: Octocrab,
}

impl Client {
    /// A client, each connection given to `reroute` first where one is
    /// set. Each call names its whole address.
    pub fn new(reroute: Option<Reroute>) -> Result<Client> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| anyhow!("making a runtime for GitHub's API: {e}"))?;
        let mut http = HttpConnector::new();
        http.enforce_http(false);
        http.set_connect_timeout(Some(TRY_FOR));
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_provider_and_webpki_roots(provider)
            .map_err(|e| anyhow!("TLS for GitHub's API: {e}"))?
            .https_or_http()
            .enable_http1()
            .wrap_connector(Connector { http, reroute });
        // octocrab's buffer starts its worker on the runtime it is made in,
        // which runs it while a call is waited for.
        let entered = runtime.enter();
        let client =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .build(https);
        let octocrab = OctocrabBuilder::new_empty()
            .with_service(client)
            .with_layer(&BudgetLayer)
            .with_layer(&TimeoutLayer::new(TRY_FOR))
            .with_layer(&GlobalConcurrencyLimitLayer::with_semaphore(GITHUB.clone()))
            .with_auth(AuthState::None)
            .build()
            .map_err(|e| anyhow!("a client of GitHub's API: {e}"))?;
        drop(entered);
        Ok(Client { runtime, octocrab })
    }

    /// `req` sent, and its answer read whole, its body within
    /// [`TRY_FOR`] of its headers; a call that got no answer read, how it
    /// stands.
    pub fn send(&self, req: Request<String>) -> Result<Raw, Failed<anyhow::Error>> {
        let octocrab = &self.octocrab;
        self.runtime.block_on(async move {
            let answer = octocrab
                .execute(req)
                .await
                .map_err(|e| Failed::new(judged(&e), anyhow::Error::new(e)))?;
            let (parts, body) = answer.into_parts();
            let budget = parts
                .extensions
                .get::<Budget>()
                .cloned()
                .unwrap_or_else(|| Budget::read(&parts.headers));
            let bytes = match tokio::time::timeout(TRY_FOR, body.collect()).await {
                Ok(Ok(b)) => b.to_bytes(),
                Ok(Err(e)) => return Err(Failed::new(Judged::Unknown, anyhow::Error::new(e))),
                Err(_) => {
                    return Err(Failed::new(
                        Judged::Unknown,
                        anyhow!("GitHub's answer took longer than {TRY_FOR:?} to read"),
                    ))
                }
            };
            Ok(Raw {
                status: parts.status.as_u16(),
                headers: parts.headers,
                budget,
                text: String::from_utf8_lossy(&bytes).into_owned(),
            })
        })
    }
}

/// How a call that got no answer stands: never judged when no connection
/// was made, and not known otherwise, since GitHub may have taken it.
fn judged(e: &(dyn std::error::Error + 'static)) -> Judged {
    use std::io::ErrorKind;
    let mut at: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(e) = at {
        if e.downcast_ref::<hyper_util::client::legacy::Error>()
            .is_some_and(|h| h.is_connect())
        {
            return Judged::Never;
        }
        if e.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                ErrorKind::ConnectionRefused
                    | ErrorKind::AddrNotAvailable
                    | ErrorKind::HostUnreachable
                    | ErrorKind::NetworkUnreachable
            )
        }) {
            return Judged::Never;
        }
        at = e.source();
    }
    Judged::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    /// forge-client, "Six calls at one time": six threads, each with a
    /// client of its own, call at one time. GitHub holds each call it
    /// takes until four are in flight, so a fifth would show beside them;
    /// four reach it at one time at most, and all six are answered.
    #[test]
    fn six_calls_at_one_time_reach_github_four_at_a_time() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let api = format!("http://{}", listener.local_addr().unwrap());
        let (arrived, in_flight, most) = (
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
        );
        {
            let (arrived, in_flight, most) = (arrived.clone(), in_flight.clone(), most.clone());
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { return };
                    let (arrived, in_flight, most) =
                        (arrived.clone(), in_flight.clone(), most.clone());
                    std::thread::spawn(move || {
                        use std::io::{BufRead, BufReader, Write};
                        let mut reader = BufReader::new(stream.try_clone().unwrap());
                        let mut line = String::new();
                        while reader.read_line(&mut line).is_ok_and(|n| n > 0) {
                            if line == "\r\n" {
                                break;
                            }
                            line.clear();
                        }
                        let n = arrived.fetch_add(1, Ordering::SeqCst) + 1;
                        let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                        most.fetch_max(now, Ordering::SeqCst);
                        // The first four wait until all four are in flight.
                        while n <= AT_ONCE && arrived.load(Ordering::SeqCst) < AT_ONCE {
                            std::thread::yield_now();
                        }
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}"
                        );
                    });
                }
            });
        }
        let calls: Vec<_> = (0..AT_ONCE + 2)
            .map(|_| {
                let url = format!("{api}/app");
                std::thread::spawn(move || {
                    let client = Client::new(None).unwrap();
                    let req = Request::get(url).body(String::new()).unwrap();
                    client.send(req).map(|raw| raw.status)
                })
            })
            .collect();
        for call in calls {
            assert_eq!(call.join().unwrap().map_err(|f| f.error).unwrap(), 200);
        }
        assert_eq!(arrived.load(Ordering::SeqCst), AT_ONCE + 2);
        assert_eq!(most.load(Ordering::SeqCst), AT_ONCE);
        assert_eq!(GITHUB.available_permits(), AT_ONCE);
    }

    /// The budget layer reads what GitHub says of its allowance from an
    /// answer's headers onto the answer.
    #[test]
    fn the_budget_is_read_from_the_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining", "0".parse().unwrap());
        headers.insert("x-ratelimit-reset", "1700000000".parse().unwrap());
        headers.insert("x-ratelimit-resource", "graphql".parse().unwrap());
        headers.insert("retry-after", "90".parse().unwrap());
        assert_eq!(
            Budget::read(&headers),
            Budget {
                remaining: Some(0),
                reset: DateTime::from_timestamp(1_700_000_000, 0),
                resource: Some("graphql".into()),
                retry_after: Some(90),
            }
        );
        assert_eq!(Budget::read(&HeaderMap::new()), Budget::default());
    }
}
