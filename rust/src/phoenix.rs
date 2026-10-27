//! Best-effort HTTP export of the same redacted JSON events written to the
//! durable audit log. No collector I/O or waiting occurs on the proxy hot path.

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bytes::Bytes;
use http::{HeaderValue, Request, StatusCode, Uri};
use http_body_util::Full;
use hyper_util::rt::TokioIo;
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_rustls::{TlsConnector, rustls::pki_types::ServerName};
use tracing_subscriber::fmt::MakeWriter;

use crate::proxy::BoxError;

#[cfg(test)]
mod tests;

const MAX_EVENT_BYTES: usize = 64 * 1024;
const MAX_BATCH_BYTES: usize = 512 * 1024;

/// Optional `[phoenix]` configuration. Tokens are read from the environment,
/// never from TOML, CLI arguments, or URLs.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PhoenixConfig {
    pub endpoint: String,
    pub token_env: String,
    pub queue_capacity: usize,
    pub batch_size: usize,
    pub flush_interval_ms: u64,
    pub request_timeout_ms: u64,
    pub shutdown_timeout_ms: u64,
    pub max_retries: u32,
}

impl Default for PhoenixConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            token_env: "LEASH_EXPORTER_TOKEN".into(),
            queue_capacity: 1024,
            batch_size: 200,
            flush_interval_ms: 1000,
            request_timeout_ms: 5000,
            shutdown_timeout_ms: 5000,
            max_retries: 3,
        }
    }
}

impl PhoenixConfig {
    pub fn validate(&self) -> Result<(), String> {
        let uri = self
            .endpoint
            .parse::<Uri>()
            .map_err(|_| "invalid phoenix.endpoint")?;
        if !matches!(uri.scheme_str(), Some("http" | "https"))
            || uri.host().is_none_or(str::is_empty)
            || uri.authority().is_some_and(|a| a.as_str().contains('@'))
            || uri
                .authority()
                .is_some_and(|a| a.as_str().len() > a.host().len() && a.port_u16().is_none())
            || uri.query().is_some()
            || self.endpoint.contains('#')
        {
            return Err("phoenix.endpoint must be an absolute HTTP(S) URL without credentials, query, or fragment".into());
        }
        if self.token_env.is_empty()
            || !self
                .token_env
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err("phoenix.token_env must name an environment variable".into());
        }
        if !(1..=65536).contains(&self.queue_capacity)
            || !(1..=200).contains(&self.batch_size)
            || !(1..=60000).contains(&self.flush_interval_ms)
            || !(1..=60000).contains(&self.request_timeout_ms)
            || !(1..=60000).contains(&self.shutdown_timeout_ms)
            || self.max_retries > 10
        {
            return Err("invalid phoenix queue, batch, timeout, or retry limits".into());
        }
        Ok(())
    }
}

/// Nonblocking tracing writer. Only attach it to the audit-event layer.
#[derive(Clone)]
pub struct PhoenixWriter {
    sender: mpsc::Sender<Vec<u8>>,
    dropped: Arc<AtomicU64>,
}

pub struct EventWriter {
    writer: PhoenixWriter,
    bytes: Vec<u8>,
    oversized: bool,
}

impl<'a> MakeWriter<'a> for PhoenixWriter {
    type Writer = EventWriter;

    fn make_writer(&'a self) -> Self::Writer {
        EventWriter {
            writer: self.clone(),
            bytes: Vec::new(),
            oversized: false,
        }
    }
}

impl Write for EventWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > MAX_EVENT_BYTES {
            self.oversized = true;
            self.bytes.clear();
        } else if !self.oversized {
            self.bytes.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for EventWriter {
    fn drop(&mut self) {
        if self.oversized
            || (!self.bytes.is_empty()
                && self
                    .writer
                    .sender
                    .try_send(std::mem::take(&mut self.bytes))
                    .is_err())
        {
            self.writer.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Owns the background task. Explicit shutdown drains within a fixed deadline;
/// dropping the guard aborts it (including any in-flight connection).
pub struct PhoenixExporter {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
    shutdown_timeout: Duration,
}

impl PhoenixExporter {
    pub fn start(config: PhoenixConfig) -> Result<(PhoenixWriter, Self), String> {
        config.validate()?;
        let token = std::env::var(&config.token_env)
            .map_err(|_| format!("{} must be set for Phoenix export", config.token_env))?;
        if token.is_empty() {
            return Err("Phoenix export token must not be empty".into());
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| "invalid Phoenix export token")?;
        authorization.set_sensitive(true);
        Self::start_authenticated(config, authorization)
    }

    fn start_authenticated(
        config: PhoenixConfig,
        authorization: HeaderValue,
    ) -> Result<(PhoenixWriter, Self), String> {
        config.validate()?;
        let uri = config.endpoint.parse::<Uri>().expect("validated URI");
        let tls = if uri.scheme_str() == Some("https") {
            let mut tls = (*crate::tls::upstream_config()
                .map_err(|_| "failed to load Phoenix TLS trust roots")?)
            .clone();
            tls.alpn_protocols = vec![b"http/1.1".to_vec()];
            Some(TlsConnector::from(Arc::new(tls)))
        } else {
            None
        };
        let (sender, receiver) = mpsc::channel(config.queue_capacity);
        let (shutdown, stopped) = oneshot::channel();
        let dropped = Arc::new(AtomicU64::new(0));
        let writer = PhoenixWriter {
            sender,
            dropped: dropped.clone(),
        };
        let shutdown_timeout = Duration::from_millis(config.shutdown_timeout_ms);
        let task = tokio::spawn(run(
            config,
            uri,
            authorization,
            tls,
            receiver,
            stopped,
            dropped,
        ));
        Ok((
            writer,
            Self {
                shutdown: Some(shutdown),
                task,
                shutdown_timeout,
            },
        ))
    }

    pub async fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        match tokio::time::timeout(self.shutdown_timeout, &mut self.task).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                tracing::warn!(target: "leash_proxy::phoenix", "Phoenix export task failed; replay retained JSONL for missing events");
            }
            Err(_) => {
                self.task.abort();
                let _ = (&mut self.task).await;
                tracing::warn!(target: "leash_proxy::phoenix", "Phoenix export shutdown timed out; replay retained JSONL for missing events");
            }
        }
    }
}

impl Drop for PhoenixExporter {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run(
    config: PhoenixConfig,
    uri: Uri,
    authorization: HeaderValue,
    tls: Option<TlsConnector>,
    mut receiver: mpsc::Receiver<Vec<u8>>,
    mut shutdown: oneshot::Receiver<()>,
    dropped: Arc<AtomicU64>,
) {
    let mut closing = false;
    loop {
        let first = tokio::select! {
            biased;
            _ = &mut shutdown, if !closing => {
                closing = true;
                receiver.close();
                continue;
            }
            event = receiver.recv() => match event { Some(event) => event, None => break },
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                report_drops(&dropped);
                continue;
            }
        };
        let deadline = tokio::time::sleep(Duration::from_millis(config.flush_interval_ms));
        tokio::pin!(deadline);
        let mut batch = vec![first];
        let mut size = batch[0].len();
        while batch.len() < config.batch_size && size + MAX_EVENT_BYTES <= MAX_BATCH_BYTES {
            tokio::select! {
                biased;
                _ = &mut shutdown, if !closing => {
                    closing = true;
                    receiver.close();
                }
                event = receiver.recv() => match event {
                    Some(event) => { size += event.len(); batch.push(event); }
                    None => break,
                },
                _ = &mut deadline => break,
            }
        }
        let count = batch.len();
        let mut body = Vec::with_capacity(size + count + 16);
        body.extend_from_slice(b"{\"events\":[");
        for (index, event) in batch.iter().enumerate() {
            if index > 0 {
                body.push(b',');
            }
            body.extend_from_slice(event.trim_ascii());
        }
        body.extend_from_slice(b"]}");
        let body = Bytes::from(body);
        let mut delivered = false;
        for attempt in 0..=config.max_retries {
            let result = tokio::time::timeout(
                Duration::from_millis(config.request_timeout_ms),
                post(&uri, &authorization, tls.as_ref(), body.clone()),
            )
            .await;
            match result {
                Ok(Ok(status)) if status.is_success() => {
                    delivered = true;
                    break;
                }
                Ok(Ok(status))
                    if !(status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS) =>
                {
                    tracing::warn!(target: "leash_proxy::phoenix", status = status.as_u16(), "Phoenix rejected audit batch");
                    break;
                }
                _ => {}
            }
            if attempt < config.max_retries {
                tokio::time::sleep(Duration::from_millis(100 * (1_u64 << attempt.min(6)))).await;
            }
        }
        if !delivered {
            dropped.fetch_add(count as u64, Ordering::Relaxed);
        }
        report_drops(&dropped);
    }
    report_drops(&dropped);
}

fn report_drops(dropped: &AtomicU64) {
    let lost = dropped.swap(0, Ordering::Relaxed);
    if lost > 0 {
        tracing::warn!(target: "leash_proxy::phoenix", dropped = lost, "Phoenix export events dropped; replay retained JSONL to recover");
    }
}

async fn post(
    uri: &Uri,
    authorization: &HeaderValue,
    tls: Option<&TlsConnector>,
    body: Bytes,
) -> Result<StatusCode, BoxError> {
    let host = uri
        .host()
        .expect("validated host")
        .trim_start_matches('[')
        .trim_end_matches(']');
    let port = uri
        .port_u16()
        .unwrap_or(if tls.is_some() { 443 } else { 80 });
    let stream = TcpStream::connect((host, port)).await?;
    if let Some(tls) = tls {
        let stream = tls
            .connect(ServerName::try_from(host.to_string())?, stream)
            .await?;
        send(stream, uri, authorization, body).await
    } else {
        send(stream, uri, authorization, body).await
    }
}

async fn send<I>(
    io: I,
    uri: &Uri,
    authorization: &HeaderValue,
    body: Bytes,
) -> Result<StatusCode, BoxError>
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(io)).await?;
    // Abort on success, failure, timeout, or shutdown: no detached connections.
    struct Connection(JoinHandle<()>);
    impl Drop for Connection {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _connection = Connection(tokio::spawn(async move {
        let _ = connection.await;
    }));
    let request = Request::post(uri.path_and_query().map_or("/", |p| p.as_str()))
        .header(
            http::header::HOST,
            uri.authority().expect("validated authority").as_str(),
        )
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::AUTHORIZATION, authorization)
        .header(http::header::CONNECTION, "close")
        .body(Full::new(body))?;
    Ok(sender.send_request(request).await?.status())
}
