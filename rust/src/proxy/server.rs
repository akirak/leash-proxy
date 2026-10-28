//! Proxy server: TCP listener, HTTP/1.1 and HTTP/2 connection handling, request
//! forwarding, and CONNECT tunneling.
//!
//! Every request is evaluated against the configured [`PolicyEngine`]
//! *before* any upstream connection is opened or bytes are forwarded; a
//! deny produces a 403 and an audit record. Forward mode tunnels CONNECT
//! traffic; transparent mode terminates TLS using an operator-supplied CA.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::header::{CONTENT_TYPE, HOST};
use http::{HeaderMap, Method, Request, Response, StatusCode, Version};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::rustls;

use super::transparent::{self, InterceptedConnection, TransparentConfig};
use crate::audit::AuditRecord;
use crate::policy::{PolicyDecision, PolicyEngine};
use crate::proxy::agent::AgentResolver;
use crate::proxy::body::{CountedBody, OnFinish, SharedCounter};
use crate::proxy::types::{ProxyRequest, Scheme, TlsInfo};
use crate::tls::CertificateAuthority;

/// Error type for forwarded bodies.
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// The body type used by all proxy responses.
pub type ProxyBody = BoxBody<Bytes, BoxError>;

/// The upstream HTTP forwarding client (request body: counted `Incoming`).
pub type ForwardingClient = Client<HttpConnector, CountedBody<Incoming>>;

/// How long the proxy waits when opening an upstream connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Headers stripped at the proxy hop and never forwarded upstream (or back
/// to the client). Also none of these are in the audit header allowlist by
/// default.
const HOP_BY_HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Shared proxy state: policy engine, agent resolver, logging allowlist, and
/// the pooled upstream forwarding client.
pub struct ProxyState {
    engine: Arc<dyn PolicyEngine>,
    resolver: Arc<dyn AgentResolver>,
    header_allowlist: Vec<String>,
    client: ForwardingClient,
    pub(super) transparent: Option<TransparentConfig>,
}

impl ProxyState {
    pub fn new(
        engine: Arc<dyn PolicyEngine>,
        resolver: Arc<dyn AgentResolver>,
        header_allowlist: Vec<String>,
    ) -> Self {
        let mut connector = HttpConnector::new();
        connector.set_connect_timeout(Some(CONNECT_TIMEOUT));
        let client: ForwardingClient = Client::builder(TokioExecutor::new()).build(connector);
        Self {
            engine,
            resolver,
            header_allowlist,
            client,
            transparent: None,
        }
    }

    /// Enable Linux REDIRECT interception with an existing CA. Upstream
    /// certificates are verified using the supplied client trust configuration.
    pub fn with_transparent(
        mut self,
        ca: Arc<CertificateAuthority>,
        upstream: Arc<rustls::ClientConfig>,
    ) -> Self {
        self.transparent = Some(TransparentConfig { ca, upstream });
        self
    }
}

/// Accept loop: every connection is handled on its own task.
pub async fn serve(listener: TcpListener, state: Arc<ProxyState>) -> Result<(), BoxError> {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(err) => {
                tracing::warn!(target: "leash_proxy::proxy", %err, "failed to accept connection");
                continue;
            }
        };
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, peer, state).await {
                tracing::warn!(target: "leash_proxy::proxy", peer = %peer, %err, "connection handler failed");
            }
        });
    }
}

async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    state: Arc<ProxyState>,
) -> Result<(), BoxError> {
    if state.transparent.is_some() {
        let destination = transparent::original_destination(&stream)?;
        let local = stream.local_addr()?;
        if destination.ip().to_canonical() == local.ip().to_canonical()
            && destination.port() == local.port()
        {
            return Err(
                "transparent connection was not redirected; refusing to proxy to itself".into(),
            );
        }
        return transparent::handle_connection(stream, peer, destination, state).await;
    }
    serve_http(stream, peer, state, None).await
}

pub(super) async fn serve_http<I>(
    stream: I,
    peer: SocketAddr,
    state: Arc<ProxyState>,
    intercepted: Option<InterceptedConnection>,
) -> Result<(), BoxError>
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let transparent = intercepted.is_some();
    let version = intercepted.as_ref().and_then(|conn| conn.http_version);
    let service = service_fn(move |req| {
        let state = state.clone();
        let intercepted = intercepted.clone();
        async move { handle_request(req, peer, state, intercepted).await }
    });
    let mut builder = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
    builder.http1().preserve_header_case(true);
    let builder = match version {
        Some(Version::HTTP_2) => builder.http2_only(),
        Some(_) => builder.http1_only(),
        None => builder,
    };
    if transparent {
        // TLS follows ALPN strictly; cleartext detects the HTTP/2 preface.
        builder
            .serve_connection(TokioIo::new(stream), service)
            .await?;
    } else {
        builder
            .serve_connection_with_upgrades(TokioIo::new(stream), service)
            .await?;
    }
    Ok(())
}

/// What the proxy should do with a request.
enum Target {
    /// `CONNECT host:port`: tunnel opaque bytes.
    Connect { host: String, port: u16 },
    /// Forwarded absolute-form HTTP request.
    Http {
        scheme: Scheme,
        host: String,
        port: u16,
        path: String,
    },
}

fn classify(req: &Request<Incoming>, intercepted: bool) -> Result<Target, &'static str> {
    if req.method() == Method::CONNECT {
        let authority = req
            .uri()
            .authority()
            .ok_or("CONNECT request without authority")?;
        let port = authority.port_u16().unwrap_or(443);
        return Ok(Target::Connect {
            host: authority.host().to_string(),
            port,
        });
    }
    let uri = req.uri();
    let scheme = match uri.scheme_str() {
        Some("http") => Scheme::Http,
        Some("https") if intercepted => Scheme::Https,
        Some("https") => return Err("https requests must be sent through a CONNECT tunnel"),
        _ => return Err("not a proxy request: use absolute-form URIs or CONNECT"),
    };
    let authority = uri.authority().ok_or("proxy request without authority")?;
    let port = authority
        .port_u16()
        .unwrap_or(if scheme == Scheme::Https { 443 } else { 80 });
    let path = uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());
    Ok(Target::Http {
        scheme,
        host: authority
            .host()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string(),
        port,
        path,
    })
}

/// Handle a single proxied request: classify, evaluate policy, then forward
/// or tunnel. Never errors: failures become 4xx/5xx responses with audit
/// records.
async fn handle_request(
    mut req: Request<Incoming>,
    peer: SocketAddr,
    state: Arc<ProxyState>,
    intercepted: Option<InterceptedConnection>,
) -> Result<Response<ProxyBody>, Infallible> {
    let started = Instant::now();
    let request_id = uuid::Uuid::new_v4().to_string();

    let tls = if intercepted
        .as_ref()
        .is_some_and(|conn| conn.tls_host.is_some())
    {
        TlsInfo::Terminated
    } else {
        TlsInfo::None
    };
    let classified = match &intercepted {
        Some(conn) => conn
            .prepare_request(&mut req)
            .and_then(|()| classify(&req, true)),
        None => classify(&req, false),
    };
    let target = match classified {
        Ok(target) => target,
        Err(reason) => {
            let mut proxy_request = origin_request(&req, &request_id, peer, &state);
            if tls == TlsInfo::Terminated {
                proxy_request.scheme = Scheme::Https;
            }
            AuditRecord::new(&proxy_request, &state.header_allowlist)
                .decision(&PolicyDecision::deny("proxy", reason))
                .tls(tls)
                .outcome(StatusCode::BAD_REQUEST.as_u16(), 0, 0, started)
                .emit();
            tracing::info!(
                target: "leash_proxy::proxy",
                request_id = %request_id,
                peer = %peer,
                reason,
                "malformed proxy request rejected"
            );
            return Ok(plain_response(StatusCode::BAD_REQUEST, reason));
        }
    };

    let (scheme, host, port, path, tls) = match &target {
        Target::Connect { host, port } => {
            (Scheme::Tunnel, host, *port, String::new(), TlsInfo::Tunnel)
        }
        Target::Http {
            scheme,
            host,
            port,
            path,
        } => (*scheme, host, *port, path.clone(), tls),
    };

    let proxy_request = ProxyRequest {
        request_id: request_id.clone(),
        agent_id: state.resolver.resolve(peer),
        source: peer,
        method: req.method().clone(),
        scheme,
        host: host.clone(),
        port,
        path: path.clone(),
        version: req.version(),
        headers: req.headers().clone(),
    };

    let decision = state.engine.evaluate(&proxy_request);
    if !decision.is_allowed() {
        let reason = decision
            .reason
            .clone()
            .unwrap_or_else(|| "denied by policy".to_string());
        AuditRecord::new(&proxy_request, &state.header_allowlist)
            .decision(&decision)
            .tls(tls)
            .outcome(StatusCode::FORBIDDEN.as_u16(), 0, 0, started)
            .emit();
        tracing::info!(
            target: "leash_proxy::proxy",
            request_id = %request_id,
            agent_id = %proxy_request.agent_id,
            host = %host,
            %reason,
            "request denied by policy"
        );
        return Ok(plain_response(
            StatusCode::FORBIDDEN,
            "blocked by leash-proxy policy",
        ));
    }

    tracing::debug!(
        target: "leash_proxy::proxy",
        request_id = %request_id,
        host = %host,
        "request allowed by policy"
    );

    match target {
        Target::Connect { host, port } => Ok(connect_response(
            req,
            proxy_request,
            decision,
            host,
            port,
            state,
            started,
        )),
        Target::Http { .. } => {
            forward_request(req, proxy_request, decision, state, started, intercepted).await
        }
    }
}

/// Build a minimal `ProxyRequest` for a request that could not be classified
/// (e.g. origin-form traffic), so the rejection is still audited.
fn origin_request(
    req: &Request<Incoming>,
    request_id: &str,
    peer: SocketAddr,
    state: &ProxyState,
) -> ProxyRequest {
    ProxyRequest {
        request_id: request_id.to_string(),
        agent_id: state.resolver.resolve(peer),
        source: peer,
        method: req.method().clone(),
        scheme: Scheme::Http,
        host: String::new(),
        port: 0,
        path: req
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/")
            .to_string(),
        version: req.version(),
        headers: req.headers().clone(),
    }
}

/// Respond to an allowed CONNECT request: reply 200, then tunnel opaque
/// bytes to the upstream host in a spawned task.
#[allow(clippy::too_many_arguments)]
fn connect_response(
    req: Request<Incoming>,
    proxy_request: ProxyRequest,
    decision: PolicyDecision,
    host: String,
    port: u16,
    state: Arc<ProxyState>,
    started: Instant,
) -> Response<ProxyBody> {
    let record = AuditRecord::new(&proxy_request, &state.header_allowlist)
        .decision(&decision)
        .tls(TlsInfo::Tunnel);
    let authority = format!("{host}:{port}");
    let on_upgrade = hyper::upgrade::on(req);

    tokio::spawn(async move {
        // TokioIo adapts hyper's upgraded connection to tokio's AsyncRead/
        // AsyncWrite for the bidirectional copy below.
        let mut client = match on_upgrade.await {
            Ok(upgraded) => TokioIo::new(upgraded),
            Err(err) => {
                record
                    .error(format!("client tunnel upgrade failed: {err}"))
                    .outcome(StatusCode::OK.as_u16(), 0, 0, started)
                    .emit();
                return;
            }
        };

        let upstream =
            tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host.as_str(), port))).await;
        let mut upstream = match upstream {
            Ok(Ok(stream)) => stream,
            Ok(Err(err)) => {
                record
                    .error(format!("failed to connect to {authority}: {err}"))
                    .outcome(StatusCode::OK.as_u16(), 0, 0, started)
                    .emit();
                let _ = client.shutdown().await;
                return;
            }
            Err(_) => {
                record
                    .error(format!("timed out connecting to {authority}"))
                    .outcome(StatusCode::OK.as_u16(), 0, 0, started)
                    .emit();
                let _ = client.shutdown().await;
                return;
            }
        };

        // The 200 response was already sent, so the client-visible status is
        // OK; any failure past this point is recorded as an error with byte
        // counts from the partially copied tunnel.
        match tokio::io::copy_bidirectional(&mut client, &mut upstream).await {
            Ok((to_upstream, to_client)) => record
                .outcome(StatusCode::OK.as_u16(), to_upstream, to_client, started)
                .emit(),
            Err(err) => record
                .error(format!("tunnel failed: {err}"))
                .outcome(StatusCode::OK.as_u16(), 0, 0, started)
                .emit(),
        }
    });

    Response::builder()
        .status(StatusCode::OK)
        .body(full_body(Bytes::new()))
        .expect("static response")
}

/// Forward an allowed absolute-form HTTP request to the upstream host.
async fn forward_request(
    req: Request<Incoming>,
    proxy_request: ProxyRequest,
    decision: PolicyDecision,
    state: Arc<ProxyState>,
    started: Instant,
    intercepted: Option<InterceptedConnection>,
) -> Result<Response<ProxyBody>, Infallible> {
    let tls = if intercepted
        .as_ref()
        .is_some_and(|conn| conn.tls_host.is_some())
    {
        TlsInfo::Terminated
    } else {
        TlsInfo::None
    };
    let (mut parts, body) = req.into_parts();

    let bytes_in: SharedCounter = Arc::new(AtomicU64::new(0));
    let bytes_out: SharedCounter = Arc::new(AtomicU64::new(0));

    // Forward-mode cleartext origins use HTTP/1.1 even when the downstream
    // client uses HTTP/2. Transparent TLS selects its upstream version via ALPN.
    if intercepted.is_none() {
        parts.version = Version::HTTP_11;
        strip_hop_by_hop(&mut parts.headers, false);
    }
    let upstream_request =
        Request::from_parts(parts, CountedBody::counting(body, bytes_in.clone()));
    let result = if let Some(connection) = intercepted {
        intercepted_request(upstream_request, &connection, &proxy_request.host, &state).await
    } else {
        state
            .client
            .request(upstream_request)
            .await
            .map_err(BoxError::from)
    };
    let upstream_response = match result {
        Ok(response) => response,
        Err(err) => {
            let sent = bytes_in.load(Ordering::Relaxed);
            AuditRecord::new(&proxy_request, &state.header_allowlist)
                .decision(&decision)
                .tls(tls)
                .error(format!("upstream request failed: {err}"))
                .outcome(StatusCode::BAD_GATEWAY.as_u16(), sent, 0, started)
                .emit();
            tracing::warn!(
                target: "leash_proxy::proxy",
                request_id = %proxy_request.request_id,
                host = %proxy_request.host,
                %err,
                "upstream request failed"
            );
            return Ok(plain_response(
                StatusCode::BAD_GATEWAY,
                "upstream request failed",
            ));
        }
    };

    let status = upstream_response.status().as_u16();
    let (mut up_parts, up_body) = upstream_response.into_parts();
    // Protocol negotiation is independent on each hop. In particular an
    // HTTP/2 origin response must not be serialized as HTTP/2 on an HTTP/1 hop.
    up_parts.version = proxy_request.version;
    strip_hop_by_hop(&mut up_parts.headers, false);

    // The audit record for this request is emitted when the response body
    // finishes streaming back to the client, so `bytes_in`/`bytes_out` are
    // complete.
    let finish = AuditRecord::new(&proxy_request, &state.header_allowlist)
        .decision(&decision)
        .tls(tls);
    let on_finish: OnFinish = Box::new(move |bytes_out_total| {
        let sent = bytes_in.load(Ordering::Relaxed);
        finish
            .outcome(status, sent, bytes_out_total, started)
            .emit();
    });
    let body = CountedBody::finishing(up_body, bytes_out, on_finish)
        .map_err(BoxError::from)
        .boxed();

    Ok(Response::from_parts(up_parts, body))
}

async fn intercepted_request(
    mut req: Request<CountedBody<Incoming>>,
    connection: &InterceptedConnection,
    host: &str,
    state: &ProxyState,
) -> Result<Response<Incoming>, BoxError> {
    let config = state
        .transparent
        .as_ref()
        .ok_or("missing transparent configuration")?;
    let (io, version) = tokio::time::timeout(
        CONNECT_TIMEOUT,
        transparent::connect_upstream(connection, host, config),
    )
    .await??;
    *req.version_mut() = version;
    strip_hop_by_hop(req.headers_mut(), version == Version::HTTP_2);
    if version == Version::HTTP_2 {
        // Keep scheme and authority for HTTP/2 pseudo-headers, but drop the
        // Host header: RFC 9113 §8.3 conveys the authority via :authority, and
        // strict origins (for example Google's GFE) reset the stream with a
        // protocol error when a Host header is sent alongside :authority.
        req.headers_mut().remove(HOST);
        let (mut sender, conn) =
            hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(io)).await?;
        tokio::spawn(async move {
            if let Err(err) = conn.await {
                tracing::debug!(target: "leash_proxy::proxy", %err, "intercepted HTTP/2 upstream connection closed");
            }
        });
        Ok(sender.send_request(req).await?)
    } else {
        *req.uri_mut() = req
            .uri()
            .path_and_query()
            .ok_or("missing request path")?
            .as_str()
            .parse()?;
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(io)).await?;
        tokio::spawn(async move {
            if let Err(err) = conn.await {
                tracing::debug!(target: "leash_proxy::proxy", %err, "intercepted upstream connection closed");
            }
        });
        Ok(sender.send_request(req).await?)
    }
}

fn strip_hop_by_hop(headers: &mut HeaderMap, preserve_te_trailers: bool) {
    // HTTP/2 forbids connection-specific headers, with one request-header
    // exception: TE: trailers (required by protocols such as gRPC).
    let te_trailers = preserve_te_trailers
        && headers
            .get(http::header::TE)
            .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"trailers"));
    let connection_headers: Vec<_> = headers
        .get_all(http::header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|name| http::header::HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect();
    for name in connection_headers {
        headers.remove(name);
    }
    for name in HOP_BY_HOP_HEADERS {
        headers.remove(*name);
    }
    if te_trailers {
        headers.insert(http::header::TE, http::HeaderValue::from_static("trailers"));
    }
}

fn full_body(bytes: Bytes) -> ProxyBody {
    Full::new(bytes)
        .map_err(|never| -> BoxError { match never {} })
        .boxed()
}

fn plain_response(status: StatusCode, message: &str) -> Response<ProxyBody> {
    let body = format!("leash-proxy: {message}\n");
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(full_body(Bytes::from(body)))
        .expect("static response parts")
}
