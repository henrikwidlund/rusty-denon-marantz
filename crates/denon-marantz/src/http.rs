//! HTTP client for the HTTP interfaces of a receiver: AppCommand, the status
//! pages and the web interface.
//!
//! Receivers speak plain HTTP on the local network. One client keeps a small
//! pool of connections to one receiver and port.

use std::error::Error as StdError;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::{Client, Error as ClientError};
use hyper_util::rt::TokioExecutor;
use tokio::time::Instant;

use crate::error::Error;
use crate::pacer::{Adaptive, Pacer, Pacing};
use crate::priority::Priority;

/// Settings of an [`HttpClient`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HttpConfig {
    /// Time allowed to open a connection.
    pub connect_timeout: Duration,
    /// Time allowed for a whole request, from sending it until the answer is read.
    /// It includes the connect time. The wait for the pacer and for commands
    /// is not counted.
    pub request_timeout: Duration,
    /// Largest answer that is accepted, in bytes.
    pub max_response_bytes: usize,
    /// Time that an unused connection stays open.
    pub idle_timeout: Duration,
    /// Spacing between requests.
    pub pacing: Pacing,
    /// How long a request tries again when the connection cannot be built,
    /// for example while the network of the caller comes up. The request was
    /// not sent then, so this is safe for commands too. A request that was
    /// sent is never sent again. [`Duration::ZERO`] (the default) does not
    /// try again. The tries stay within `request_timeout`. A short
    /// `connect_timeout` makes a lost connect attempt end sooner, so that the
    /// next try starts sooner.
    pub connect_retry: Duration,
}

/// Pause before the first new try to build a connection. It doubles with each
/// try, up to [`LAST_CONNECT_PAUSE`].
const FIRST_CONNECT_PAUSE: Duration = Duration::from_millis(50);

/// Longest pause between two tries to build a connection.
const LAST_CONNECT_PAUSE: Duration = Duration::from_millis(250);

impl HttpConfig {
    /// The same settings with another time for a whole request, but not more
    /// than the current one.
    #[must_use]
    pub const fn with_request_timeout(mut self, timeout: Duration) -> Self {
        if timeout.as_nanos() < self.request_timeout.as_nanos() {
            self.request_timeout = timeout;
        }
        self
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(2),
            request_timeout: Duration::from_secs(15),
            max_response_bytes: 4 * 1024 * 1024,
            idle_timeout: Duration::from_secs(30),
            pacing: Pacing::Adaptive(Adaptive::default()),
            connect_retry: Duration::ZERO,
        }
    }
}

/// Connections to one receiver on one port.
#[derive(Debug)]
pub struct HttpClient {
    client: Client<HttpConnector, Full<Bytes>>,
    authority: hyper::http::uri::Authority,
    config: HttpConfig,
    pacer: Arc<Pacer>,
    /// Makes the requests wait for the commands, if set.
    priority: Option<Arc<Priority>>,
    /// The requests also wait for the pause after the last command.
    pause: bool,
}

impl HttpClient {
    /// Creates a client. It does not connect yet.
    ///
    /// # Errors
    ///
    /// Returns an error if the host is not valid or a setting is out of range.
    pub fn new(host: &str, port: u16, config: HttpConfig) -> Result<Self, Error> {
        let authority = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let authority = authority
            .parse()
            .map_err(|e| Error::InvalidRequest(format!("host {host:?}: {e}")))?;
        let mut connector = HttpConnector::new();
        connector.set_connect_timeout(Some(config.connect_timeout));
        connector.set_nodelay(true);
        let client = Client::builder(TokioExecutor::new())
            .pool_idle_timeout(config.idle_timeout)
            .pool_max_idle_per_host(3)
            .build(connector);
        Ok(Self {
            client,
            authority,
            config,
            pacer: Arc::new(Pacer::new(config.pacing)?),
            priority: None,
            pause: true,
        })
    }

    /// The requests wait for the commands of `priority`. Commands are sent
    /// with [`HttpClient::command`].
    pub(crate) fn with_priority(mut self, priority: Arc<Priority>) -> Self {
        self.priority = Some(priority);
        self
    }

    /// The settings of this client.
    pub(crate) const fn config(&self) -> HttpConfig {
        self.config
    }

    /// A client that shares the connections, the pacer and the commands of
    /// this one. Its requests wait for the commands, but not for the pause
    /// after them.
    pub(crate) fn without_pause(&self) -> Self {
        Self {
            client: self.client.clone(),
            authority: self.authority.clone(),
            config: self.config,
            pacer: Arc::clone(&self.pacer),
            priority: self.priority.clone(),
            pause: false,
        }
    }

    /// Host and port of the receiver.
    #[must_use]
    pub fn authority(&self) -> &str {
        self.authority.as_str()
    }

    /// Sends a GET request and returns the body of the answer.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, times out, or the receiver
    /// answers with an error status or a body above the limit.
    pub async fn get(&self, path: &str) -> Result<Bytes, Error> {
        self.send(Method::GET, path, Bytes::new()).await
    }

    /// Sends a POST request and returns the body of the answer.
    ///
    /// # Errors
    ///
    /// See [`HttpClient::get`].
    pub async fn post(&self, path: &str, body: Bytes) -> Result<Bytes, Error> {
        self.send(Method::POST, path, body).await
    }

    /// Sends a GET request and reads the answer with `parse`.
    ///
    /// `parse` makes the value from the body, for example
    /// [`DeviceInfo::parse`](denon_marantz_proto::DeviceInfo::parse). Its error
    /// is wrapped in [`Error::Protocol`] with the request.
    ///
    /// # Errors
    ///
    /// See [`HttpClient::get`]. Also returns an error if `parse` does.
    pub async fn get_parsed<T>(
        &self,
        path: &str,
        parse: impl FnOnce(&[u8]) -> Result<T, denon_marantz_proto::Error>,
    ) -> Result<T, Error> {
        let body = self.get(path).await?;
        parse(&body).map_err(|source| Error::Protocol {
            request: self.describe(&Method::GET, path),
            source,
        })
    }

    /// Sends a POST request and reads the answer with `parse`.
    ///
    /// # Errors
    ///
    /// See [`HttpClient::get_parsed`].
    pub async fn post_parsed<T>(
        &self,
        path: &str,
        body: Bytes,
        parse: impl FnOnce(&[u8]) -> Result<T, denon_marantz_proto::Error>,
    ) -> Result<T, Error> {
        let answer = self.post(path, body).await?;
        parse(&answer).map_err(|source| Error::Protocol {
            request: self.describe(&Method::POST, path),
            source,
        })
    }

    /// Sends a command as a GET request. Unlike the other requests, it does
    /// not wait for the commands. The caller holds the turn of the command.
    pub(crate) async fn command(&self, path: &str) -> Result<Bytes, Error> {
        self.pacer.wait().await;
        self.timed(Method::GET, path, Bytes::new()).await
    }

    async fn send(&self, method: Method, path: &str, body: Bytes) -> Result<Bytes, Error> {
        self.wait_turn().await;
        self.timed(method, path, body).await
    }

    /// Waits for the pacer and, if set, for the commands. A command that comes
    /// while the request waits for the pacer goes first.
    async fn wait_turn(&self) {
        let Some(priority) = &self.priority else {
            return self.pacer.wait().await;
        };
        loop {
            priority.read(self.pause).await;
            self.pacer.wait().await;
            if priority.read_allowed(self.pause) {
                return;
            }
        }
    }

    /// Sends a request with the time limit, after the wait.
    async fn timed(&self, method: Method, path: &str, body: Bytes) -> Result<Bytes, Error> {
        tracing::trace!(host = self.authority(), %method, path, "request");
        let result = tokio::time::timeout(
            self.config.request_timeout,
            self.exchange(&method, path, body),
        )
        .await
        .unwrap_or_else(|_| {
            Err(Error::Timeout {
                request: self.describe(&method, path),
                limit: self.config.request_timeout,
                // The limit can end a connect too, but it is not known which.
                sent: true,
            })
        });
        match result {
            Ok((answer, try_started)) => {
                // From the try that worked: the tries before it say nothing
                // about the speed of the receiver.
                self.pacer.record_latency(try_started);
                Ok(answer)
            }
            Err(error) => {
                tracing::debug!(host = self.authority(), %method, path, %error, "request failed");
                Err(error)
            }
        }
    }

    /// The answer, and when the try that got it started.
    async fn exchange(
        &self,
        method: &Method,
        path: &str,
        body: Bytes,
    ) -> Result<(Bytes, Instant), Error> {
        let uri = Uri::builder()
            .scheme("http")
            .authority(self.authority.clone())
            .path_and_query(path)
            .build()
            .map_err(|e| invalid_path(path, &e))?;
        let (response, try_started) = self.request(method, path, &uri, body).await?;
        let status = response.status();
        if !status.is_success() {
            let request = self.describe(method, path);
            return Err(if status == StatusCode::FORBIDDEN {
                Error::Forbidden { request }
            } else {
                Error::Status {
                    request,
                    status: status.as_u16(),
                }
            });
        }
        let limit = self.config.max_response_bytes;
        let collected = Limited::new(response.into_body(), limit)
            .collect()
            .await
            .map_err(|e| self.classify_body(e, method, path))?;
        Ok((collected.to_bytes(), try_started))
    }

    /// Sends the request. A connection that cannot be built is tried again
    /// within [`HttpConfig::connect_retry`]. Also returns when the try that
    /// worked started.
    async fn request(
        &self,
        method: &Method,
        path: &str,
        uri: &Uri,
        body: Bytes,
    ) -> Result<(Response<Incoming>, Instant), Error> {
        // `None` if the limit is too large for a time: no end then.
        let deadline = Instant::now().checked_add(self.config.connect_retry);
        let mut pause = FIRST_CONNECT_PAUSE;
        loop {
            let request = Request::builder()
                .method(method.clone())
                .uri(uri.clone())
                .body(Full::new(body.clone()))
                .map_err(|e| invalid_path(path, &e))?;
            let try_started = Instant::now();
            match self.client.request(request).await {
                Ok(response) => return Ok((response, try_started)),
                // The request was not sent, so it can be sent again.
                Err(error)
                    if error.is_connect()
                        && deadline.is_none_or(|deadline| Instant::now() + pause <= deadline) =>
                {
                    tracing::debug!(host = self.authority(), %method, path, %error, "no connection, trying again");
                    tokio::time::sleep(pause).await;
                    pause = (pause * 2).min(LAST_CONNECT_PAUSE);
                }
                Err(error) => return Err(self.classify(error, method, path)),
            }
        }
    }

    fn describe(&self, method: &Method, path: &str) -> String {
        format!("{method} http://{}{path}", self.authority)
    }

    fn classify(&self, error: ClientError, method: &Method, path: &str) -> Error {
        let request = self.describe(method, path);
        // A connect error means that nothing was written.
        let sent = !error.is_connect();
        let mut source: Option<&(dyn StdError + 'static)> = Some(&error);
        while let Some(cause) = source {
            if let Some(io) = cause.downcast_ref::<io::Error>()
                && io.kind() == io::ErrorKind::TimedOut
            {
                return Error::Timeout {
                    request,
                    limit: self.config.connect_timeout,
                    sent,
                };
            }
            // A connection that is closed before the answer is a network
            // error, so that another try is made. A garbled answer is not.
            if let Some(hyper) = cause.downcast_ref::<hyper::Error>()
                && (hyper.is_parse() || hyper.is_parse_status())
            {
                return Error::InvalidResponse {
                    request,
                    source: Box::new(error),
                };
            }
            source = cause.source();
        }
        Error::Network {
            request,
            source: Box::new(error),
            sent,
        }
    }

    fn classify_body(
        &self,
        error: Box<dyn StdError + Send + Sync>,
        method: &Method,
        path: &str,
    ) -> Error {
        let request = self.describe(method, path);
        if error.downcast_ref::<LengthLimitError>().is_some() {
            return Error::ResponseTooLarge {
                request,
                limit: self.config.max_response_bytes,
            };
        }
        match error.downcast::<hyper::Error>() {
            Ok(hyper) if hyper.is_incomplete_message() || hyper.is_parse() => {
                Error::InvalidResponse {
                    request,
                    source: hyper,
                }
            }
            Ok(hyper) => Error::Network {
                request,
                source: hyper,
                sent: true,
            },
            Err(other) => Error::Network {
                request,
                source: other,
                sent: true,
            },
        }
    }
}

fn invalid_path(path: &str, error: &dyn std::fmt::Display) -> Error {
    Error::InvalidRequest(format!("path {path:?}: {error}"))
}
