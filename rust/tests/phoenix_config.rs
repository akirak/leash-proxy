use clap::Parser;
use leash_proxy::cli::Cli;
use leash_proxy::config::ProxyConfig;
use leash_proxy::phoenix::PhoenixConfig;

#[test]
fn disabled_by_default_and_cli_overrides_file_endpoint() {
    let config = ProxyConfig::load(&Cli::try_parse_from(["leash-proxy"]).unwrap()).unwrap();
    assert!(config.phoenix.is_none());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[phoenix]\nendpoint = 'http://localhost:4000/api/audit/events'\nqueue_capacity = 12\n",
    )
    .unwrap();
    let cli = Cli::try_parse_from([
        "leash-proxy",
        "--config",
        path.to_str().unwrap(),
        "--exporter-url",
        "https://collector.example/api/audit/events",
    ])
    .unwrap();
    let config = ProxyConfig::load(&cli).unwrap().phoenix.unwrap();
    assert_eq!(
        config.endpoint,
        "https://collector.example/api/audit/events"
    );
    assert_eq!(config.queue_capacity, 12);
    assert_eq!(config.token_env, "LEASH_EXPORTER_TOKEN");
}

#[test]
fn rejects_invalid_urls_and_limits() {
    for endpoint in [
        "",
        "/api/audit/events",
        "ftp://localhost/events",
        "http://secret@localhost/events",
        "http://localhost/#secret",
        "http://localhost/events?token=secret",
        "http://localhost:abc/events",
        "http://localhost:/events",
        "http://localhost:99999/events",
    ] {
        assert!(
            PhoenixConfig {
                endpoint: endpoint.into(),
                ..Default::default()
            }
            .validate()
            .is_err(),
            "accepted {endpoint}"
        );
    }
    for endpoint in [
        "http://127.0.0.1:4000/api/audit/events",
        "https://collector.example/events",
        "http://[::1]:4000/events",
    ] {
        assert!(
            PhoenixConfig {
                endpoint: endpoint.into(),
                ..Default::default()
            }
            .validate()
            .is_ok(),
            "rejected {endpoint}"
        );
    }
    for setting in [
        "queue_capacity = 0",
        "batch_size = 0",
        "batch_size = 201",
        "flush_interval_ms = 0",
        "request_timeout_ms = 0",
        "shutdown_timeout_ms = 0",
        "max_retries = 11",
        "unknown = true",
        "token_env = ''",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            format!("[phoenix]\nendpoint = 'http://localhost/events'\n{setting}\n"),
        )
        .unwrap();
        let cli = Cli::try_parse_from(["leash-proxy", "--config", path.to_str().unwrap()]).unwrap();
        assert!(ProxyConfig::load(&cli).is_err(), "accepted {setting}");
    }
}

#[test]
fn missing_or_invalid_tokens_fail_without_exposing_secrets() {
    for token in [None, Some(""), Some("secret\ninvalid-header")] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_leash-proxy"));
        command.args([
            "--exporter-url",
            "http://localhost/events",
            "--listen",
            "not-a-socket",
        ]);
        command.env_remove("LEASH_EXPORTER_TOKEN");
        if let Some(token) = token {
            command.env("LEASH_EXPORTER_TOKEN", token);
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains("failed to initialize Phoenix export"),
            "{stderr}"
        );
        assert!(!stderr.contains("secret"), "{stderr}");
    }
}
