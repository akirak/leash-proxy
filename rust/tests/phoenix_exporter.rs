//! Real binary wiring: exporter, JSONL, audit filtering, and redaction.
use std::process::Stdio;
use std::time::Duration;

use bytes::Bytes;
use http::{Request, Response};
use http_body_util::{BodyExt, Full};
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn binary_exports_only_redacted_audits_even_with_rust_log_off() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let collector = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/api/audit/events", collector.local_addr().unwrap());
        let received = tokio::spawn(async move {
            let (stream, _) = collector.accept().await.unwrap();
            let (tx, rx) = tokio::sync::oneshot::channel();
            let tx = std::sync::Mutex::new(Some(tx));
            let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                let tx = tx.lock().unwrap().take().unwrap();
                async move {
                    assert_eq!(req.headers()["authorization"], "Bearer export-test-token");
                    let body = req.into_body().collect().await.unwrap().to_bytes();
                    tx.send(body).unwrap();
                    Ok::<_, std::convert::Infallible>(Response::new(Full::new(Bytes::new())))
                }
            });
            hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(stream), service).await.unwrap();
            rx.await.unwrap()
        });
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let log = dir.path().join("audit.jsonl");
        std::fs::write(&config, format!("[phoenix]\nendpoint = {endpoint:?}\nbatch_size = 1\n")).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = port.local_addr().unwrap();
        drop(port);
        // Ready operational logs are useful for synchronization; the dedicated
        // audit layer must still work with a more restrictive general filter.
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_leash-proxy"))
            .args(["--config", config.to_str().unwrap(), "--listen", &addr.to_string(),
                "--log-file", log.to_str().unwrap()])
            .env("LEASH_EXPORTER_TOKEN", "export-test-token")
            .env("RUST_LOG", "off")
            .stderr(Stdio::piped()).kill_on_drop(true).spawn().unwrap();
        // Readiness without sending any request (and hence without generating
        // a spurious audit record) by waiting for a successful TCP connection.
        let mut stream = loop {
            match TcpStream::connect(addr).await {
                Ok(stream) => break stream,
                Err(_) => { tokio::time::sleep(Duration::from_millis(10)).await; }
            }
        };
        stream.write_all(b"POST http://admin.internal/private?token=query-secret HTTP/1.1\r\nHost: admin.internal\r\nAuthorization: Bearer request-secret\r\nCookie: cookie-secret\r\nUser-Agent: safe-agent\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 403"), "{response}");
        let body = received.await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let events = json["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event["target"], "leash_proxy::audit");
        assert_eq!(event["decision"], "deny");
        assert_eq!(event["path"], "/private");
        assert_eq!(event["status"], 403);
        assert!(event["timestamp"].is_string());
        let raw = String::from_utf8(body.to_vec()).unwrap();
        for secret in ["query-secret", "request-secret", "cookie-secret", "export-test-token"] {
            assert!(!raw.contains(secret), "leaked {secret}");
        }
        // The file writer runs independently. Wait for its single audit line.
        let mut lines = BufReader::new(tokio::fs::File::open(&log).await.unwrap()).lines();
        let file_event = loop {
            if let Some(line) = lines.next_line().await.unwrap() {
                break serde_json::from_str::<serde_json::Value>(&line).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert_eq!(file_event["request_id"], event["request_id"]);
        for key in ["path", "headers", "decision", "status", "latency_ms", "target"] {
            assert_eq!(file_event[key], event[key], "mismatch in {key}");
        }
        child.kill().await.unwrap();
    }).await.expect("binary export completes without blocking proxy traffic");
}
