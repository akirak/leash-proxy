use super::*;
use std::convert::Infallible;

use http::Response;
use http_body_util::BodyExt;
use hyper::service::service_fn;
use tokio::net::TcpListener;

fn enqueue(writer: &PhoenixWriter, event: &[u8]) {
    writer.make_writer().write_all(event).unwrap();
}

#[test]
fn writer_is_bounded_and_drops_only_export_copies() {
    let (sender, mut receiver) = mpsc::channel(1);
    let dropped = Arc::new(AtomicU64::new(0));
    let writer = PhoenixWriter {
        sender,
        dropped: dropped.clone(),
    };
    {
        let mut event = writer.make_writer();
        event.write_all(b"{\"one\":").unwrap();
        event.write_all(b"1}\n").unwrap();
    }
    enqueue(&writer, b"{\"two\":2}\n");
    assert_eq!(dropped.load(Ordering::Relaxed), 1);
    assert_eq!(receiver.try_recv().unwrap(), b"{\"one\":1}\n");
    enqueue(&writer, &vec![b'x'; MAX_EVENT_BYTES + 1]);
    assert_eq!(dropped.load(Ordering::Relaxed), 2);
    assert!(receiver.try_recv().is_err());
    drop(receiver);
    enqueue(&writer, b"{}");
    assert_eq!(dropped.load(Ordering::Relaxed), 3);
}

async fn collector(
    statuses: Vec<StatusCode>,
) -> (
    String,
    mpsc::Receiver<(http::request::Parts, Bytes)>,
    JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/api/audit/events", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel(32);
    let task = tokio::spawn(async move {
        for status in statuses {
            let (stream, _) = listener.accept().await.unwrap();
            let tx = tx.clone();
            let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                let tx = tx.clone();
                async move {
                    let (parts, body) = req.into_parts();
                    let body = body.collect().await.unwrap().to_bytes();
                    tx.send((parts, body)).await.unwrap();
                    Ok::<_, Infallible>(
                        Response::builder()
                            .status(status)
                            .body(Full::new(Bytes::new()))
                            .unwrap(),
                    )
                }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await;
        }
    });
    (endpoint, rx, task)
}

fn exporter(endpoint: String, batch_size: usize) -> (PhoenixWriter, PhoenixExporter) {
    PhoenixExporter::start_authenticated(
        PhoenixConfig {
            endpoint,
            batch_size,
            flush_interval_ms: 10,
            ..Default::default()
        },
        HeaderValue::from_static("Bearer test-secret"),
    )
    .unwrap()
}

#[tokio::test]
async fn batches_events_and_retries_identical_payload_on_503_and_429() {
    let (endpoint, mut requests, server) = collector(vec![
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::OK,
    ])
    .await;
    let (writer, exporter) = exporter(endpoint, 2);
    enqueue(&writer, b"{\"request_id\":\"one\"}\n");
    enqueue(&writer, b"{\"request_id\":\"two\"}\n");
    let mut bodies = Vec::new();
    for _ in 0..3 {
        let (parts, body) = tokio::time::timeout(Duration::from_secs(2), requests.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(parts.method, http::Method::POST);
        assert_eq!(parts.uri, "/api/audit/events");
        assert_eq!(parts.headers["authorization"], "Bearer test-secret");
        assert_eq!(parts.headers["content-type"], "application/json");
        bodies.push(body);
    }
    assert_eq!(bodies[0], bodies[1]);
    assert_eq!(bodies[1], bodies[2]);
    let json: serde_json::Value = serde_json::from_slice(&bodies[0]).unwrap();
    assert_eq!(json["events"].as_array().unwrap().len(), 2);
    assert_eq!(json["events"][0]["request_id"], "one");
    exporter.shutdown().await;
    server.await.unwrap();
}

#[tokio::test]
async fn flushes_partial_batch_on_timer_and_shutdown_with_writer_still_alive() {
    let (endpoint, mut requests, server) = collector(vec![StatusCode::OK; 2]).await;
    let (writer, exporter) = exporter(endpoint, 200);
    enqueue(&writer, b"{\"request_id\":\"timer\"}");
    let (_, body) = tokio::time::timeout(Duration::from_secs(2), requests.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&body).contains("timer"));
    enqueue(&writer, b"{\"request_id\":\"shutdown\"}");
    exporter.shutdown().await;
    let (_, body) = requests.recv().await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("shutdown"));
    assert!(writer.sender.is_closed());
    server.await.unwrap();
}

#[tokio::test]
async fn permanent_failure_is_not_retried_and_next_batch_can_succeed() {
    let (endpoint, mut requests, server) =
        collector(vec![StatusCode::UNAUTHORIZED, StatusCode::OK]).await;
    let (writer, exporter) = exporter(endpoint, 1);
    enqueue(&writer, b"{\"request_id\":\"rejected\"}");
    enqueue(&writer, b"{\"request_id\":\"next\"}");
    exporter.shutdown().await;
    let (_, first) = requests.recv().await.unwrap();
    let (_, second) = requests.recv().await.unwrap();
    assert!(String::from_utf8_lossy(&first).contains("rejected"));
    assert!(String::from_utf8_lossy(&second).contains("next"));
    server.await.unwrap();
}

#[tokio::test]
async fn retry_limit_advances_to_next_batch() {
    let (endpoint, mut requests, server) =
        collector(vec![StatusCode::SERVICE_UNAVAILABLE; 2]).await;
    let (writer, exporter) = PhoenixExporter::start_authenticated(
        PhoenixConfig {
            endpoint,
            batch_size: 1,
            max_retries: 0,
            ..Default::default()
        },
        HeaderValue::from_static("Bearer test"),
    )
    .unwrap();
    enqueue(&writer, b"{\"request_id\":1}");
    enqueue(&writer, b"{\"request_id\":2}");
    exporter.shutdown().await;
    assert_ne!(
        requests.recv().await.unwrap().1,
        requests.recv().await.unwrap().1
    );
    server.await.unwrap();
}

#[tokio::test]
async fn hung_collector_cannot_prevent_bounded_shutdown() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/api/audit/events", listener.local_addr().unwrap());
    let (writer, exporter) = PhoenixExporter::start_authenticated(
        PhoenixConfig {
            endpoint,
            batch_size: 1,
            shutdown_timeout_ms: 50,
            ..Default::default()
        },
        HeaderValue::from_static("Bearer test"),
    )
    .unwrap();
    enqueue(&writer, b"{}");
    let (_socket, _) = listener.accept().await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), exporter.shutdown())
        .await
        .unwrap();
    assert!(writer.sender.is_closed());
}

#[test]
fn audit_layer_excludes_operational_events() {
    use tracing_subscriber::{Layer, layer::SubscriberExt};
    let (sender, mut receiver) = mpsc::channel(8);
    let writer = PhoenixWriter {
        sender,
        dropped: Arc::new(AtomicU64::new(0)),
    };
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_writer(writer)
            .with_filter(tracing_subscriber::EnvFilter::new(
                "leash_proxy::audit=info",
            )),
    );
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(target: "leash_proxy", "ready");
        tracing::error!(target: "leash_proxy::phoenix", "export failure");
        tracing::info!(target: "leash_proxy::audit", request_id = "only-audit", "request");
    });
    let event: serde_json::Value = serde_json::from_slice(&receiver.try_recv().unwrap()).unwrap();
    assert_eq!(event["request_id"], "only-audit");
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn https_verifies_collector_certificate() {
    use crate::tls::{
        CertificateAuthority,
        tests::{fixture, trusting},
    };
    let (dir, cert) = fixture();
    let (_other_dir, other_cert) = fixture();
    let ca = CertificateAuthority::load(dir.path()).unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(ca.server_config("127.0.0.1").unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri: Uri = format!(
        "https://{}/api/audit/events",
        listener.local_addr().unwrap()
    )
    .parse()
    .unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        assert!(acceptor.accept(socket).await.is_err());
        let (socket, _) = listener.accept().await.unwrap();
        let stream = acceptor.accept(socket).await.unwrap();
        let service = service_fn(|req: Request<hyper::body::Incoming>| async move {
            assert_eq!(req.headers()["authorization"], "Bearer test");
            assert_eq!(
                req.into_body().collect().await.unwrap().to_bytes(),
                "{\"events\":[]}"
            );
            Ok::<_, Infallible>(Response::new(Full::new(Bytes::new())))
        });
        hyper::server::conn::http1::Builder::new()
            .serve_connection(TokioIo::new(stream), service)
            .await
            .unwrap();
    });
    let authorization = HeaderValue::from_static("Bearer test");
    let body = Bytes::from_static(b"{\"events\":[]}");
    assert!(
        post(
            &uri,
            &authorization,
            Some(&TlsConnector::from(trusting(&other_cert))),
            body.clone()
        )
        .await
        .is_err()
    );
    assert_eq!(
        post(
            &uri,
            &authorization,
            Some(&TlsConnector::from(trusting(&cert))),
            body
        )
        .await
        .unwrap(),
        StatusCode::OK
    );
    server.await.unwrap();
}

#[tokio::test]
async fn request_timeout_moves_past_a_hung_collector() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/api/audit/events", listener.local_addr().unwrap());
    let (writer, exporter) = PhoenixExporter::start_authenticated(
        PhoenixConfig {
            endpoint,
            batch_size: 1,
            request_timeout_ms: 20,
            max_retries: 0,
            ..Default::default()
        },
        HeaderValue::from_static("Bearer test"),
    )
    .unwrap();
    enqueue(&writer, b"{}");
    let (_socket, _) = listener.accept().await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), exporter.shutdown())
        .await
        .unwrap();
    assert!(writer.sender.is_closed());
}
