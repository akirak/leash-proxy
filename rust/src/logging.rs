//! Structured logging setup (ADR 0001/0002).
//!
//! All output is JSON lines. Audit events (target `leash_proxy::audit`) are
//! always emitted at INFO level regardless of `RUST_LOG`, so the durable
//! audit trail cannot be silenced accidentally; operational logging honors
//! `RUST_LOG` with a default of INFO.

use std::fs::{OpenOptions, create_dir_all};
use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{Layer, fmt};

const AUDIT_TARGET: &str = "leash_proxy::audit";

/// Install the global subscriber. The returned guard must be kept alive
/// for the life of the process (it flushes the background writer).
pub fn init(log_file: Option<&Path>) -> Result<WorkerGuard, String> {
    let (writer, guard) = match log_file {
        Some(path) => {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
                && let Err(err) = create_dir_all(parent)
            {
                return Err(format!(
                    "failed to create log directory {}: {err}",
                    parent.display()
                ));
            }
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|err| format!("failed to open log file {}: {err}", path.display()))?;
            tracing_appender::non_blocking(file)
        }
        None => tracing_appender::non_blocking(std::io::stdout()),
    };

    // Audit records flow unconditionally at INFO; operational logs honor
    // RUST_LOG (default INFO) and are kept out of the audit stream.
    let audit_filter = EnvFilter::new(format!("{AUDIT_TARGET}=info"));
    let mut general_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let directive: tracing_subscriber::filter::Directive = format!("{AUDIT_TARGET}=off")
        .parse()
        .map_err(|err| format!("invalid log directive: {err}"))?;
    general_filter = general_filter.add_directive(directive);

    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .json()
                .flatten_event(true)
                .with_current_span(false)
                .with_span_list(false)
                .with_writer(writer.clone())
                .with_filter(general_filter),
        )
        .with(
            fmt::layer()
                .json()
                .flatten_event(true)
                .with_current_span(false)
                .with_span_list(false)
                .with_writer(writer)
                .with_filter(audit_filter),
        )
        .init();

    Ok(guard)
}
