use clap::Parser;
use leash_proxy::cli::{Cli, ProxyMode};
use leash_proxy::config::ProxyConfig;
use std::path::PathBuf;

#[test]
fn forward_mode_remains_default_without_ca() {
    let config = ProxyConfig::load(&Cli::try_parse_from(["leash-proxy"]).unwrap()).unwrap();
    assert_eq!(config.mode, ProxyMode::Forward);
    assert!(config.ca_dir.is_none());
}

#[test]
fn ca_directory_cli_overrides_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[server]\nmode = 'forward'\nca_dir = '/file/ca'\n").unwrap();
    let cli = Cli::try_parse_from(["leash-proxy", "--config", path.to_str().unwrap()]).unwrap();
    assert_eq!(
        ProxyConfig::load(&cli).unwrap().ca_dir,
        Some(PathBuf::from("/file/ca"))
    );
    let cli = Cli::try_parse_from([
        "leash-proxy",
        "--config",
        path.to_str().unwrap(),
        "--ca-dir",
        "/cli/ca",
    ])
    .unwrap();
    assert_eq!(
        ProxyConfig::load(&cli).unwrap().ca_dir,
        Some(PathBuf::from("/cli/ca"))
    );
}

#[test]
fn transparent_mode_requires_ca() {
    let cli = Cli::try_parse_from(["leash-proxy", "--mode", "transparent"]).unwrap();
    assert!(ProxyConfig::load(&cli).is_err());
    let cli =
        Cli::try_parse_from(["leash-proxy", "--mode", "transparent", "--ca-dir", "/ca"]).unwrap();
    assert_eq!(ProxyConfig::load(&cli).is_ok(), cfg!(target_os = "linux"));
}

#[test]
fn mode_cli_overrides_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[server]\nmode = 'transparent'\nca_dir = '/ca'\n").unwrap();
    let cli = Cli::try_parse_from(["leash-proxy", "--config", path.to_str().unwrap()]).unwrap();
    if cfg!(target_os = "linux") {
        assert_eq!(
            ProxyConfig::load(&cli).unwrap().mode,
            ProxyMode::Transparent
        );
    } else {
        assert!(ProxyConfig::load(&cli).is_err());
    }
    let cli = Cli::try_parse_from([
        "leash-proxy",
        "--config",
        path.to_str().unwrap(),
        "--mode",
        "forward",
    ])
    .unwrap();
    assert_eq!(ProxyConfig::load(&cli).unwrap().mode, ProxyMode::Forward);
}

#[test]
fn invalid_ca_stops_binary_before_binding() {
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_leash-proxy"))
        .args([
            "--ca-dir",
            dir.path().to_str().unwrap(),
            "--listen",
            "not-a-socket",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let logs = String::from_utf8(output.stdout).unwrap();
    assert!(logs.contains("failed to load CA"), "{logs}");
    assert!(!logs.contains("failed to bind listener"), "{logs}");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
