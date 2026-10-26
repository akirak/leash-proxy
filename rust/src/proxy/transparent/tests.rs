use super::*;
use crate::policy::{PolicyDecision, PolicyEngine};
use crate::proxy::{ProxyRequest, Scheme, StaticAgentResolver};
use crate::tls::tests::{fixture, trusting};
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

mod http2;

#[derive(Default)]
struct TestPolicy(Mutex<Vec<ProxyRequest>>);
impl PolicyEngine for TestPolicy {
    fn name(&self) -> &'static str {
        "test"
    }
    fn evaluate(&self, request: &ProxyRequest) -> PolicyDecision {
        self.0.lock().unwrap().push(request.clone());
        if request.method == Method::GET {
            PolicyDecision::allow("test", vec![])
        } else {
            PolicyDecision::deny("test", "only GET allowed")
        }
    }
}

fn state(
    ca: Arc<CertificateAuthority>,
    upstream: Arc<rustls::ClientConfig>,
) -> (Arc<ProxyState>, Arc<TestPolicy>) {
    let policy = Arc::new(TestPolicy::default());
    let resolver = Arc::new(StaticAgentResolver::from_entries(&Default::default()));
    let state = ProxyState::new(policy.clone(), resolver, vec!["host".to_string()])
        .with_transparent(ca, upstream);
    (Arc::new(state), policy)
}

// Supply the recovered destination directly so the interception path can be
// tested over real TCP without root, netfilter rules, or network namespaces.
async fn interceptor(state: Arc<ProxyState>, destination: SocketAddr) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (stream, peer) = listener.accept().await.unwrap();
        let _ = handle_connection(stream, peer, destination, state).await;
    });
    addr
}

async fn http_echo<I: AsyncRead + AsyncWrite + Unpin>(mut io: I) {
    let mut head = Vec::new();
    let mut byte = [0];
    while io.read_exact(&mut byte).await.is_ok() {
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8(head).unwrap();
    let first_line = head.lines().next().unwrap();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{first_line}",
        first_line.len()
    );
    io.write_all(response.as_bytes()).await.unwrap();
    io.shutdown().await.unwrap();
}

async fn exchange<I: AsyncRead + AsyncWrite + Unpin>(mut io: I, request: &str) -> String {
    io.write_all(request.as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut head = Vec::new();
        let mut byte = [0];
        while !head.ends_with(b"\r\n\r\n") {
            io.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }
        let head = String::from_utf8(head).unwrap();
        let len: usize = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .expect("response content length");
        let mut body = vec![0; len];
        io.read_exact(&mut body).await.unwrap();
        format!("{head}{}", String::from_utf8(body).unwrap())
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn transparent_http_preserves_destination_and_evaluates_request() {
    let (dir, cert) = fixture();
    let ca = Arc::new(CertificateAuthority::load(dir.path()).unwrap());
    let (state, policy) = state(ca, trusting(&cert));
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination = upstream.local_addr().unwrap();
    tokio::spawn(async move {
        http_echo(upstream.accept().await.unwrap().0).await;
    });
    let addr = interceptor(state, destination).await;
    let response = exchange(
        TcpStream::connect(addr).await.unwrap(),
        "GET /health?token=secret HTTP/1.1\r\nHost: example.test\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with("GET /health?token=secret HTTP/1.1"));
    let requests = policy.0.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].scheme, Scheme::Http);
    assert_eq!(requests[0].host, "example.test");
    assert_eq!(requests[0].port, destination.port());
}

#[tokio::test]
async fn transparent_https_uses_byo_ca_and_checks_decrypted_requests() {
    for host in ["example.test", "127.0.0.1"] {
        let (dir, cert) = fixture();
        let ca = Arc::new(CertificateAuthority::load(dir.path()).unwrap());
        let client = trusting(&cert);
        let (state, policy) = state(ca.clone(), client.clone());
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let destination = upstream.local_addr().unwrap();
        let acceptor = TlsAcceptor::from(ca.server_config(host).unwrap());
        tokio::spawn(async move {
            let tls = acceptor
                .accept(upstream.accept().await.unwrap().0)
                .await
                .unwrap();
            http_echo(tls).await;
        });
        let addr = interceptor(state, destination).await;
        let stream = TcpStream::connect(addr).await.unwrap();
        let tls = TlsConnector::from(client)
            .connect(ServerName::try_from(host.to_string()).unwrap(), stream)
            .await
            .unwrap();
        // The client performs normal CA and DNS/IP SAN verification. IP
        // ServerName omits SNI, exercising destination-IP fallback too.
        let leaf = tls.get_ref().1.peer_certificates().unwrap()[0].clone();
        let leaf = openssl::x509::X509::from_der(leaf.as_ref()).unwrap();
        assert!(leaf.verify(&cert.public_key().unwrap()).unwrap());
        let request = format!(
            "GET /repos?token=secret HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        );
        let response = exchange(tls, &request).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.ends_with("GET /repos?token=secret HTTP/1.1"));
        let requests = policy.0.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].scheme, Scheme::Https);
        assert_eq!(requests[0].host, host);
        assert_eq!(requests[0].port, destination.port());
        assert_eq!(requests[0].path, "/repos?token=secret");
    }
}

#[tokio::test]
async fn denies_before_opening_upstream_and_rejects_host_sni_mismatch() {
    for (method, host, status, evaluated) in [
        ("POST", "example.test", "403", true),
        ("GET", "different.test", "400", false),
        ("CONNECT", "example.test", "400", false),
    ] {
        let (dir, cert) = fixture();
        let ca = Arc::new(CertificateAuthority::load(dir.path()).unwrap());
        let client = trusting(&cert);
        let (state, policy) = state(ca, client.clone());
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = interceptor(state, upstream.local_addr().unwrap()).await;
        let stream = TcpStream::connect(addr).await.unwrap();
        let tls = TlsConnector::from(client)
            .connect(ServerName::try_from("example.test").unwrap(), stream)
            .await
            .unwrap();
        let path = if method == "CONNECT" {
            "example.test:443"
        } else {
            "/repos"
        };
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let response = exchange(tls, &request).await;
        assert!(
            response.starts_with(&format!("HTTP/1.1 {status}")),
            "{response}"
        );
        assert_eq!(!policy.0.lock().unwrap().is_empty(), evaluated);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), upstream.accept())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn rejects_untrusted_upstream_certificate() {
    let (dir, cert) = fixture();
    let (other_dir, _) = fixture();
    let ca = Arc::new(CertificateAuthority::load(dir.path()).unwrap());
    let other_ca = CertificateAuthority::load(other_dir.path()).unwrap();
    let client = trusting(&cert);
    let (state, _) = state(ca, client.clone());
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination = upstream.local_addr().unwrap();
    let acceptor = TlsAcceptor::from(other_ca.server_config("example.test").unwrap());
    tokio::spawn(async move {
        let _ = acceptor.accept(upstream.accept().await.unwrap().0).await;
    });
    let addr = interceptor(state, destination).await;
    let stream = TcpStream::connect(addr).await.unwrap();
    let tls = TlsConnector::from(client)
        .connect(ServerName::try_from("example.test").unwrap(), stream)
        .await
        .unwrap();
    let response = exchange(
        tls,
        "GET / HTTP/1.1\r\nHost: example.test\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 502"), "{response}");
}
