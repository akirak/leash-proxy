use super::*;
use bytes::Bytes;
use http::{HeaderMap, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::convert::Infallible;

use crate::proxy::{BoxError, ProxyBody};

fn body(bytes: impl Into<Bytes>) -> ProxyBody {
    Full::new(bytes.into()).map_err(BoxError::from).boxed()
}

fn alpn_client(cert: &openssl::x509::X509, protocols: &[&[u8]]) -> Arc<rustls::ClientConfig> {
    let mut config = (*trusting(cert)).clone();
    config.alpn_protocols = protocols.iter().map(|protocol| protocol.to_vec()).collect();
    Arc::new(config)
}

async fn h2_client<I>(io: I) -> hyper::client::conn::http2::SendRequest<ProxyBody>
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(io))
            .await
            .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    sender
}

async fn tls_client(
    addr: SocketAddr,
    client: Arc<rustls::ClientConfig>,
) -> tokio_rustls::client::TlsStream<TcpStream> {
    tokio::time::timeout(Duration::from_secs(5), async {
        TlsConnector::from(client)
            .connect(
                ServerName::try_from("example.test").unwrap(),
                TcpStream::connect(addr).await.unwrap(),
            )
            .await
            .unwrap()
    })
    .await
    .unwrap()
}

fn request(method: Method, uri: String) -> Request<ProxyBody> {
    Request::builder()
        .method(method)
        .uri(uri)
        .version(Version::HTTP_2)
        .body(body("payload"))
        .unwrap()
}

async fn response(
    sender: &mut hyper::client::conn::http2::SendRequest<ProxyBody>,
    request: Request<ProxyBody>,
) -> (StatusCode, Bytes, Option<HeaderMap>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let response = sender.send_request(request).await.unwrap();
        assert_eq!(response.version(), Version::HTTP_2);
        for name in ["connection", "x-hop", "transfer-encoding", "upgrade"] {
            assert!(
                !response.headers().contains_key(name),
                "{name} forwarded to HTTP/2 client"
            );
        }
        let status = response.status();
        let collected = response.into_body().collect().await.unwrap();
        let trailers = collected.trailers().cloned();
        (status, collected.to_bytes(), trailers)
    })
    .await
    .unwrap()
}

#[derive(Debug)]
struct Observed {
    version: Version,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
    trailers: Option<HeaderMap>,
}

async fn echo(
    req: Request<Incoming>,
    observed: Arc<Mutex<Vec<Observed>>>,
) -> Result<Response<ProxyBody>, Infallible> {
    let (parts, body) = req.into_parts();
    let collected = body.collect().await.unwrap();
    let trailers = collected.trailers().cloned();
    let bytes = collected.to_bytes();
    observed.lock().unwrap().push(Observed {
        version: parts.version,
        uri: parts.uri,
        headers: parts.headers,
        body: bytes.clone(),
        trailers: trailers.clone(),
    });
    let body = Full::new(bytes)
        .with_trailers(std::future::ready(trailers.map(Ok)))
        .map_err(BoxError::from)
        .boxed();
    let mut response = Response::new(body);
    if parts.version != Version::HTTP_2 {
        response
            .headers_mut()
            .insert("connection", HeaderValue::from_static("x-hop"));
        response
            .headers_mut()
            .insert("x-hop", HeaderValue::from_static("must-not-forward"));
    }
    Ok(response)
}

/// TLS origins use a fixed protocol, proving the proxy follows upstream ALPN
/// independently of the downstream protocol. None exercises no-ALPN fallback.
async fn tls_origin(
    ca: &CertificateAuthority,
    version: Option<Version>,
) -> (
    SocketAddr,
    Arc<Mutex<Vec<Observed>>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let mut server = (*ca.server_config("example.test").unwrap()).clone();
    server.alpn_protocols = match version {
        Some(Version::HTTP_2) => vec![b"h2".to_vec()],
        Some(_) => vec![b"http/1.1".to_vec()],
        None => vec![],
    };
    let acceptor = TlsAcceptor::from(Arc::new(server));
    let observations = observed.clone();
    let task = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let acceptor = acceptor.clone();
            let observations = observations.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else {
                    return;
                };
                let expected_alpn = match version {
                    Some(Version::HTTP_2) => Some(b"h2".as_slice()),
                    Some(_) => Some(b"http/1.1".as_slice()),
                    None => None,
                };
                assert_eq!(tls.get_ref().1.alpn_protocol(), expected_alpn);
                let service = service_fn(move |req| echo(req, observations.clone()));
                if version == Some(Version::HTTP_2) {
                    let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                        .serve_connection(TokioIo::new(tls), service)
                        .await;
                } else {
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(tls), service)
                        .await;
                }
            });
        }
    });
    (addr, observed, task)
}

#[tokio::test]
async fn negotiates_http2_and_http1_independently_on_each_tls_hop() {
    for downstream in [Version::HTTP_2, Version::HTTP_11] {
        for upstream in [Some(Version::HTTP_2), Some(Version::HTTP_11), None] {
            let (dir, cert) = fixture();
            let ca = Arc::new(CertificateAuthority::load(dir.path()).unwrap());
            let (state, policy) = state(ca.clone(), alpn_client(&cert, &[b"h2", b"http/1.1"]));
            let (destination, observed, task) = tls_origin(&ca, upstream).await;
            let addr = interceptor(state, destination).await;
            let alpn = if downstream == Version::HTTP_2 {
                b"h2".as_slice()
            } else {
                b"http/1.1".as_slice()
            };
            let tls = tls_client(addr, alpn_client(&cert, &[alpn])).await;
            assert_eq!(tls.get_ref().1.alpn_protocol(), Some(alpn));
            if downstream == Version::HTTP_2 {
                let mut sender = h2_client(tls).await;
                let uri = format!(
                    "https://example.test:{}/repos?secret=token",
                    destination.port()
                );
                let (status, bytes, _) = response(&mut sender, request(Method::GET, uri)).await;
                assert_eq!(status, StatusCode::OK);
                assert_eq!(bytes, "payload");
            } else {
                let response = exchange(tls,
                    "GET /repos?secret=token HTTP/1.1\r\nHost: example.test\r\nContent-Length: 7\r\nConnection: close\r\n\r\npayload").await;
                assert!(response.starts_with("HTTP/1.1 200"), "{response}");
                assert!(
                    !response.to_ascii_lowercase().contains("x-hop"),
                    "{response}"
                );
                assert!(response.ends_with("payload"), "{response}");
            }
            let requests = policy.0.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].version, downstream);
            assert_eq!(requests[0].scheme, Scheme::Https);
            assert_eq!(requests[0].host, "example.test");
            assert_eq!(requests[0].port, destination.port());
            assert_eq!(requests[0].path, "/repos?secret=token");
            assert!(requests[0].headers.contains_key(HOST));
            let origin_requests = observed.lock().unwrap();
            assert_eq!(origin_requests.len(), 1);
            assert_eq!(
                origin_requests[0].version,
                upstream.unwrap_or(Version::HTTP_11)
            );
            assert_eq!(
                origin_requests[0].uri.path_and_query().unwrap(),
                "/repos?secret=token"
            );
            assert_eq!(origin_requests[0].body, "payload");
            if upstream == Some(Version::HTTP_2) {
                assert_eq!(origin_requests[0].uri.scheme_str(), Some("https"));
                assert_eq!(
                    origin_requests[0].uri.authority().unwrap().port_u16(),
                    Some(destination.port())
                );
            }
            task.abort();
        }
    }
}

#[tokio::test]
async fn multiplexed_streams_are_independently_checked_without_opening_denied_origins() {
    let (dir, cert) = fixture();
    let ca = Arc::new(CertificateAuthority::load(dir.path()).unwrap());
    let (state, policy) = state(ca.clone(), alpn_client(&cert, &[b"h2"]));
    let (destination, observed, task) = tls_origin(&ca, Some(Version::HTTP_2)).await;
    let addr = interceptor(state, destination).await;
    let tls = tls_client(addr, alpn_client(&cert, &[b"h2"])).await;
    let sender = h2_client(tls).await;
    let uri = format!("https://example.test:{}/repos", destination.port());
    let mut requests = vec![
        (request(Method::GET, uri.clone()), StatusCode::OK),
        (request(Method::GET, uri.clone()), StatusCode::OK),
        (request(Method::POST, uri.clone()), StatusCode::FORBIDDEN),
        (
            request(Method::GET, uri.replace("example.test", "different.test")),
            StatusCode::BAD_REQUEST,
        ),
        (
            request(Method::GET, "https://example.test:1/repos".into()),
            StatusCode::BAD_REQUEST,
        ),
        (
            request(Method::GET, uri.replace("https:", "http:")),
            StatusCode::BAD_REQUEST,
        ),
    ];
    let mut mismatch = request(Method::GET, uri);
    mismatch
        .headers_mut()
        .insert(HOST, HeaderValue::from_static("different.test"));
    requests.push((mismatch, StatusCode::BAD_REQUEST));
    let mut tasks = Vec::new();
    for (request, expected) in requests {
        let mut sender = sender.clone();
        tasks.push(tokio::spawn(async move {
            let (status, _, _) = response(&mut sender, request).await;
            assert_eq!(status, expected);
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let requests = policy.0.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|req| req.version == Version::HTTP_2));
    assert_ne!(requests[0].request_id, requests[1].request_id);
    assert_eq!(observed.lock().unwrap().len(), 2);
    task.abort();
}

#[tokio::test]
async fn http2_streams_large_bodies_and_trailers_in_both_directions() {
    let (dir, cert) = fixture();
    let ca = Arc::new(CertificateAuthority::load(dir.path()).unwrap());
    let (state, _) = state(ca.clone(), alpn_client(&cert, &[b"h2"]));
    let (destination, observed, task) = tls_origin(&ca, Some(Version::HTTP_2)).await;
    let addr = interceptor(state, destination).await;
    let mut sender = h2_client(tls_client(addr, alpn_client(&cert, &[b"h2"])).await).await;
    let bytes = Bytes::from(vec![b'x'; 256 * 1024]); // Larger than the default HTTP/2 flow-control window.
    let mut trailers = HeaderMap::new();
    trailers.insert("grpc-status", HeaderValue::from_static("0"));
    let payload = Full::new(bytes.clone())
        .with_trailers(std::future::ready(Some(Ok(trailers.clone()))))
        .map_err(BoxError::from)
        .boxed();
    let request = Request::builder()
        .version(Version::HTTP_2)
        .method(Method::GET)
        .uri(format!(
            "https://example.test:{}/stream",
            destination.port()
        ))
        .header("te", "trailers")
        .body(payload)
        .unwrap();
    let (status, echoed, received_trailers) = response(&mut sender, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(echoed, bytes);
    assert_eq!(received_trailers, Some(trailers.clone()));
    let observed = observed.lock().unwrap();
    assert_eq!(observed[0].body, bytes);
    assert_eq!(observed[0].trailers, Some(trailers));
    assert_eq!(observed[0].headers.get("te").unwrap(), "trailers");
    task.abort();
}

#[tokio::test]
async fn cleartext_http2_prior_knowledge_forwards_to_http1_origin() {
    let (dir, cert) = fixture();
    let ca = Arc::new(CertificateAuthority::load(dir.path()).unwrap());
    let (state, policy) = state(ca, trusting(&cert));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination = listener.local_addr().unwrap();
    tokio::spawn(async move {
        http_echo(listener.accept().await.unwrap().0).await;
    });
    let addr = interceptor(state, destination).await;
    let mut sender = h2_client(TcpStream::connect(addr).await.unwrap()).await;
    let request = Request::builder()
        .version(Version::HTTP_2)
        .uri(format!("http://example.test:{}/health", destination.port()))
        .body(body(""))
        .unwrap();
    let (status, bytes, _) = response(&mut sender, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, "GET /health HTTP/1.1");
    let requests = policy.0.lock().unwrap();
    assert_eq!(requests[0].version, Version::HTTP_2);
    assert_eq!(requests[0].scheme, Scheme::Http);
}

#[tokio::test]
async fn http2_still_rejects_untrusted_upstream_tls() {
    let (dir, cert) = fixture();
    let (other_dir, _) = fixture();
    let ca = Arc::new(CertificateAuthority::load(dir.path()).unwrap());
    let other_ca = CertificateAuthority::load(other_dir.path()).unwrap();
    let (state, _) = state(ca, alpn_client(&cert, &[b"h2"]));
    let (destination, observed, task) = tls_origin(&other_ca, Some(Version::HTTP_2)).await;
    let addr = interceptor(state, destination).await;
    let mut sender = h2_client(tls_client(addr, alpn_client(&cert, &[b"h2"])).await).await;
    let (status, _, _) = response(
        &mut sender,
        request(
            Method::GET,
            format!("https://example.test:{}/", destination.port()),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(observed.lock().unwrap().is_empty());
    task.abort();
}

#[test]
fn invalid_http2_authorities_and_connect_are_rejected() {
    let conn = InterceptedConnection {
        destination: "127.0.0.1:443".parse().unwrap(),
        tls_host: Some("example.test".into()),
        http_version: Some(Version::HTTP_2),
    };
    for (method, uri, host) in [
        (Method::GET, "/path", None),
        (Method::CONNECT, "example.test:443", None),
        (Method::GET, "https://user@example.test/path", None),
        (
            Method::GET,
            "https://example.test/path",
            Some("example.test:invalid"),
        ),
        (
            Method::GET,
            "https://example.test/path",
            Some("example.test:444"),
        ),
    ] {
        let mut req = Request::builder()
            .version(Version::HTTP_2)
            .method(method)
            .uri(uri)
            .body(())
            .unwrap();
        if let Some(host) = host {
            req.headers_mut()
                .insert(HOST, HeaderValue::from_static(host));
        }
        assert!(conn.prepare_request(&mut req).is_err(), "{req:?}");
    }
}
