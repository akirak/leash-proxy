//! Command line interface.

use std::path::PathBuf;

use clap::Parser;

/// Policy-enforcing egress proxy for AI agents (see ADR 0001).
#[derive(Debug, Parser)]
#[command(name = "leash-proxy", version, about)]
pub struct Cli {
    /// Path to a TOML configuration file.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Socket to listen on, e.g. `0.0.0.0:8080` [default: 127.0.0.1:8080]
    #[arg(long)]
    pub listen: Option<String>,

    /// Cedar schema file (cedar schema language).
    #[arg(long)]
    pub schema: Option<PathBuf>,

    /// Cedar policy file, in Cedar text or Cedar JSON (EST) format;
    /// repeat or pass multiple values for multiple files. A file whose
    /// content starts with `{` is loaded as JSON.
    #[arg(long = "policies")]
    pub policies: Option<Vec<PathBuf>>,

    /// Append structured JSON logs to this file instead of stdout.
    #[arg(long)]
    pub log_file: Option<PathBuf>,

    /// Header name that may be recorded in audit logs; repeats allowed.
    /// Overrides the config file and the default allowlist.
    #[arg(long = "header-allowlist")]
    pub header_allowlist: Option<Vec<String>>,
}
