//! Runtime configuration: TOML file plus CLI overrides.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use crate::cli::{Cli, ProxyMode};
use crate::redact::DEFAULT_HEADER_ALLOWLIST;

/// File-based configuration (`--config`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    pub server: ServerConfig,
    /// Static mapping of source IP to agent ID, e.g. `"10.0.0.5" = "vm-alpha"`.
    pub agents: BTreeMap<String, String>,
    pub policy: PolicyFiles,
    pub logging: LoggingConfig,
    pub phoenix: Option<crate::phoenix::PhoenixConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    /// Proxy listen socket.
    pub listen: String,
    pub mode: ProxyMode,
    /// Existing PEM CA certificate and private key directory.
    pub ca_dir: Option<PathBuf>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8080".to_string(),
            mode: ProxyMode::Forward,
            ca_dir: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PolicyFiles {
    /// Cedar schema file.
    pub schema: PathBuf,
    /// Cedar policy files, in Cedar text or Cedar JSON (EST) format,
    /// evaluated as one policy set.
    pub policies: Vec<PathBuf>,
}

impl Default for PolicyFiles {
    fn default() -> Self {
        Self {
            schema: PathBuf::from("policies/schema.cedar"),
            policies: vec![PathBuf::from("policies/policies.cedar")],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    /// Append-only JSONL log file; stdout when absent.
    pub log_file: Option<PathBuf>,
    /// Header names that may appear in audit records.
    pub header_allowlist: Vec<String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            log_file: None,
            header_allowlist: DEFAULT_HEADER_ALLOWLIST
                .iter()
                .map(|name| name.to_string())
                .collect(),
        }
    }
}

/// Fully resolved runtime configuration (CLI overrides file overrides
/// defaults).
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub listen: String,
    pub mode: ProxyMode,
    pub ca_dir: Option<PathBuf>,
    pub agents: BTreeMap<String, String>,
    pub schema: PathBuf,
    pub policies: Vec<PathBuf>,
    pub log_file: Option<PathBuf>,
    pub header_allowlist: Vec<String>,
    pub phoenix: Option<crate::phoenix::PhoenixConfig>,
}

impl ProxyConfig {
    pub fn load(cli: &Cli) -> Result<Self, String> {
        let mut file = FileConfig::default();
        if let Some(path) = &cli.config {
            let raw = fs::read_to_string(path)
                .map_err(|err| format!("failed to read config file {}: {err}", path.display()))?;
            file = toml::from_str(&raw)
                .map_err(|err| format!("failed to parse config file {}: {err}", path.display()))?;
        }

        let listen = cli.listen.clone().unwrap_or(file.server.listen);
        let mode = cli.mode.unwrap_or(file.server.mode);
        let ca_dir = cli.ca_dir.clone().or(file.server.ca_dir);
        if mode == ProxyMode::Transparent {
            if !cfg!(target_os = "linux") {
                return Err("transparent mode is only supported on Linux".to_string());
            }
            if ca_dir.is_none() {
                return Err("transparent mode requires --ca-dir (or server.ca_dir)".to_string());
            }
        }
        let schema = cli.schema.clone().unwrap_or(file.policy.schema);
        let policies = cli
            .policies
            .clone()
            .filter(|policies| !policies.is_empty())
            .unwrap_or(file.policy.policies);
        if policies.is_empty() {
            return Err("no cedar policy files configured".to_string());
        }
        let log_file = cli.log_file.clone().or(file.logging.log_file);
        let header_allowlist = cli
            .header_allowlist
            .clone()
            .filter(|allowlist| !allowlist.is_empty())
            .unwrap_or(file.logging.header_allowlist);
        if header_allowlist.is_empty() {
            return Err("header allowlist must not be empty".to_string());
        }

        let mut phoenix = file.phoenix;
        if let Some(endpoint) = &cli.exporter_url {
            phoenix.get_or_insert_with(Default::default).endpoint = endpoint.clone();
        }
        if let Some(config) = &phoenix {
            config.validate()?;
        }

        let agents: BTreeMap<String, String> = file
            .agents
            .into_iter()
            .filter(|(ip, _)| ip.parse::<std::net::IpAddr>().is_ok())
            .collect();

        Ok(Self {
            listen,
            mode,
            ca_dir,
            agents,
            schema,
            policies,
            log_file,
            header_allowlist,
            phoenix,
        })
    }
}
