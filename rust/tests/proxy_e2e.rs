//! End-to-end proxy tests over real TCP: plain HTTP forwarding, CONNECT
//! tunneling, and policy denies — with a live upstream and a live proxy.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use common::{POLICIES, SCHEMA};
use leash_proxy::policy::{CedarEngine, EngineStack, PolicyEngine};
use leash_proxy::proxy::{ProxyState, StaticAgentResolver, serve};

/// Minimal HTTP upstream: echoes the request line back in a fixed response.
async fn spawn_http_upstream() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut request = Vec::new();
                loop {
                    let n = match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    request.extend_from_slice(&buf[..n]);
                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let first_line = String::from_utf8_lossy(&request)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                let body = format!("echo: {first_line}");
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nX-Upstream: leash-test\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(response.as_bytes()).await;
            });
        }
    });
    addr
}

/// Minimal TCP echo server for CONNECT tunnel tests.
async fn spawn_echo_upstream() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                loop {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if sock.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
    });
    addr
}

/// Spawn a proxy bound to an ephemeral port with the test policy set.
async fn spawn_proxy() -> SocketAddr {
    let cedar = CedarEngine::from_sources(SCHEMA, POLICIES);
    assert!(cedar.is_ready());
    let engine: Arc<dyn PolicyEngine> = Arc::new(EngineStack::new(vec![Arc::new(cedar)]));
    // Map the loopback test source to vm-alpha, which the policies grant.
    let mut agents = std::collections::BTreeMap::new();
    agents.insert("127.0.0.1".to_string(), "vm-alpha".to_string());
    let resolver = Arc::new(StaticAgentResolver::from_entries(&agents));
    let state = Arc::new(ProxyState::new(engine, resolver, vec!["host".to_string()]));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = serve(listener, state).await;
    });
    addr
}

/// Send a raw request to the proxy and read everything until close.
async fn raw_exchange(proxy: SocketAddr, request: &str) -> String {
    let mut sock = TcpStream::connect(proxy).await.unwrap();
    sock.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), sock.read_to_end(&mut response))
        .await
        .expect("proxy response within timeout")
        .expect("read proxy response");
    String::from_utf8_lossy(&response).into_owned()
}

/// Read exactly one HTTP response head (up to `\r\n\r\n`) from a socket.
async fn read_head(sock: &mut TcpStream) -> String {
    let mut response = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = sock.read(&mut byte).await.expect("read response byte");
        if n == 0 {
            break;
        }
        response.push(byte[0]);
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&response).into_owned()
}

#[tokio::test]
async fn forwards_allowed_http_request() {
    let upstream = spawn_http_upstream().await;
    let proxy = spawn_proxy().await;

    let request = format!(
        "GET http://{upstream}/repos/leash?token=sekrit HTTP/1.1\r\nHost: {upstream}\r\nUser-Agent: agent-test\r\nConnection: close\r\n\r\n"
    );
    let response = raw_exchange(proxy, &request).await;

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "expected 200, got: {response}"
    );
    assert!(
        response.contains("x-upstream: leash-test"),
        "upstream headers forwarded"
    );
    // The upstream echoes the request line it received; it must have been
    // rewritten to origin-form (no absolute URI, no hop-by-hop headers).
    assert!(
        response.contains("echo: GET /repos/leash?token=sekrit HTTP/1.1"),
        "origin-form rewrite: {response}"
    );
}

#[tokio::test]
async fn denies_http_request_not_permitted_by_policy() {
    let upstream = spawn_http_upstream().await;
    let proxy = spawn_proxy().await;

    // vm-alpha is only permitted GET to 127.0.0.1; a POST is denied.
    let request = format!(
        "POST http://{upstream}/repos HTTP/1.1\r\nHost: {upstream}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let response = raw_exchange(proxy, &request).await;
    assert!(
        response.starts_with("HTTP/1.1 403"),
        "expected 403, got: {response}"
    );
    assert!(response.contains("blocked by leash-proxy policy"));
}

#[tokio::test]
async fn rejects_origin_form_requests() {
    let _upstream = spawn_http_upstream().await;
    let proxy = spawn_proxy().await;

    // Origin-form request (no scheme/authority): not a proxy request.
    let request =
        "GET /healthz HTTP/1.1\r\nHost: something\r\nConnection: close\r\n\r\n".to_string();
    let response = raw_exchange(proxy, &request).await;
    assert!(
        response.starts_with("HTTP/1.1 400"),
        "expected 400, got: {response}"
    );
}

#[tokio::test]
async fn tunnels_allowed_connect_request() {
    let echo = spawn_echo_upstream().await;
    let proxy = spawn_proxy().await;

    let mut sock = TcpStream::connect(proxy).await.unwrap();
    let connect = format!("CONNECT {echo} HTTP/1.1\r\nHost: {echo}\r\n\r\n");
    sock.write_all(connect.as_bytes()).await.unwrap();
    let head = read_head(&mut sock).await;
    assert!(
        head.starts_with("HTTP/1.1 200"),
        "expected tunnel established, got: {head}"
    );

    // Send bytes through the tunnel and expect them echoed back.
    sock.write_all(b"leash-tunnel-ping").await.unwrap();
    let mut echoed = [0u8; 32];
    let n = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut echoed))
        .await
        .expect("echo within timeout")
        .expect("read echo");
    assert_eq!(&echoed[..n], b"leash-tunnel-ping");
}

#[tokio::test]
async fn denies_connect_to_forbidden_host() {
    let _echo = spawn_echo_upstream().await;
    let proxy = spawn_proxy().await;

    let mut sock = TcpStream::connect(proxy).await.unwrap();
    let connect = "CONNECT admin.internal:443 HTTP/1.1\r\nHost: admin.internal:443\r\n\r\n";
    sock.write_all(connect.as_bytes()).await.unwrap();
    let head = read_head(&mut sock).await;
    assert!(
        head.starts_with("HTTP/1.1 403"),
        "expected 403 for forbidden CONNECT, got: {head}"
    );
}

#[tokio::test]
async fn connect_to_unreachable_upstream_closes_tunnel() {
    // Reserve an ephemeral port, then stop listening so connects to it fail.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_port = listener.local_addr().unwrap().port();
    drop(listener);

    let proxy = spawn_proxy().await;
    let mut sock = TcpStream::connect(proxy).await.unwrap();
    let connect =
        format!("CONNECT 127.0.0.1:{dead_port} HTTP/1.1\r\nHost: 127.0.0.1:{dead_port}\r\n\r\n");
    sock.write_all(connect.as_bytes()).await.unwrap();
    let head = read_head(&mut sock).await;
    assert!(
        head.starts_with("HTTP/1.1 200"),
        "policy allows the CONNECT, tunnel established: {head}"
    );

    // The upstream connect fails; the proxy must close the client side.
    let mut buf = [0u8; 16];
    let n = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut buf))
        .await
        .expect("tunnel close within timeout")
        .expect("read from tunnel");
    assert_eq!(n, 0, "tunnel closed after upstream connect failure");
}
