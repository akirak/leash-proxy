//! Proxy server: TCP listener, HTTP/1.1 proxy connection handling, request
//! forwarding, and CONNECT tunneling.
//!
//! Every request is evaluated against the configured [`PolicyEngine`]
//! *before* any upstream connection is opened or bytes are forwarded; a
//! deny produces a 403 and an audit record. Tunneling forwards opaque bytes
//! after a CONNECT allow decision. (TLS MITM termination is a future
//! enhancement; the request/response and audit types already carry scheme
//! and TLS metadata for it.)

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::header::CONTENT_TYPE;
use http::{HeaderMap, Method, Request, Response, StatusCode};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

use crate::audit::AuditRecord;
use crate::policy::{PolicyDecision, PolicyEngine};
use crate::proxy::agent::AgentResolver;
use crate::proxy::body::{CountedBody, OnFinish, SharedCounter};
use crate::proxy::types::{ProxyRequest, Scheme, TlsInfo};

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
        }
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
    let service = service_fn(move |req| {
        let state = state.clone();
        async move { handle_request(req, peer, state).await }
    });
    hyper::server::conn::http1::Builder::new()
        .preserve_header_case(true)
        .serve_connection(TokioIo::new(stream), service)
        .with_upgrades()
        .await?;
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

fn classify(req: &Request<Incoming>) -> Result<Target, &'static str> {
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
        Some("https") => return Err("https requests must be sent through a CONNECT tunnel"),
        _ => return Err("not a proxy request: use absolute-form URIs or CONNECT"),
    };
    let authority = uri.authority().ok_or("proxy request without authority")?;
    let port = authority.port_u16().unwrap_or(80);
    let path = uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());
    Ok(Target::Http {
        scheme,
        host: authority.host().to_string(),
        port,
        path,
    })
}

/// Handle a single proxied request: classify, evaluate policy, then forward
/// or tunnel. Never errors: failures become 4xx/5xx responses with audit
/// records.
async fn handle_request(
    req: Request<Incoming>,
    peer: SocketAddr,
    state: Arc<ProxyState>,
) -> Result<Response<ProxyBody>, Infallible> {
    let started = Instant::now();
    let request_id = uuid::Uuid::new_v4().to_string();

    let target = match classify(&req) {
        Ok(target) => target,
        Err(reason) => {
            let proxy_request = origin_request(&req, &request_id, peer, &state);
            AuditRecord::new(&proxy_request, &state.header_allowlist)
                .decision(&PolicyDecision::deny("proxy", reason))
                .tls(TlsInfo::None)
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
        } => (*scheme, host, *port, path.clone(), TlsInfo::None),
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
        Target::Http { .. } => forward_request(req, proxy_request, decision, state, started).await,
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
) -> Result<Response<ProxyBody>, Infallible> {
    let (mut parts, body) = req.into_parts();

    // The absolute-form URI is kept as-is: the forwarding client uses its
    // authority to select the upstream connection, and rewrites it to
    // origin-form when speaking HTTP/1.1 to the origin server.
    strip_hop_by_hop(&mut parts.headers);

    let bytes_in: SharedCounter = Arc::new(AtomicU64::new(0));
    let bytes_out: SharedCounter = Arc::new(AtomicU64::new(0));

    let upstream_request =
        Request::from_parts(parts, CountedBody::counting(body, bytes_in.clone()));
    let upstream_response = match state.client.request(upstream_request).await {
        Ok(response) => response,
        Err(err) => {
            let sent = bytes_in.load(Ordering::Relaxed);
            AuditRecord::new(&proxy_request, &state.header_allowlist)
                .decision(&decision)
                .tls(TlsInfo::None)
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
    strip_hop_by_hop(&mut up_parts.headers);

    // The audit record for this request is emitted when the response body
    // finishes streaming back to the client, so `bytes_in`/`bytes_out` are
    // complete.
    let finish = AuditRecord::new(&proxy_request, &state.header_allowlist)
        .decision(&decision)
        .tls(TlsInfo::None);
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

fn strip_hop_by_hop(headers: &mut HeaderMap) {
    for name in HOP_BY_HOP_HEADERS {
        headers.remove(*name);
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
