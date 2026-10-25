//! Structured audit records for proxied requests (ADR 0001/0002).
//!
//! One record is emitted per request via `tracing` (target
//! `leash_proxy::audit`) so the append-only JSONL log remains the durable
//! source of truth. The record carries everything the future Elixir
//! collector and monitoring UI will index: identities, request shape
//! (redacted), the policy decision with determining policy IDs, transfer
//! stats, and TLS metadata.

use std::time::Instant;

use crate::policy::PolicyDecision;
use crate::proxy::{ProxyRequest, TlsInfo};
use crate::redact;

/// A structured audit record for a single proxied request.
#[derive(Debug, Clone)]
pub struct AuditRecord {
    pub request_id: String,
    pub agent_id: String,
    pub source_ip: String,
    pub method: String,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    /// Redacted path: query strings and fragments are stripped.
    pub path: String,
    /// Allowlisted, redacted request headers.
    pub headers: String,

    // Decision
    pub decision: String,
    pub engine: String,
    /// Comma-joined policy IDs that determined the decision.
    pub policy_ids: String,
    pub deny_reason: String,

    // Outcome
    pub status: u16,
    pub bytes_in: u64,
    pub bytes_out: u64,
    /// Milliseconds elapsed between request arrival and record emission.
    pub latency_ms: f64,
    pub tls: String,
    /// Transport error, if any (e.g. upstream connect failure).
    pub error: String,
}

impl AuditRecord {
    /// Start building a record from an observed request.
    ///
    /// Sensitive data is redacted immediately: the path loses its query
    /// string and only allowlisted headers survive.
    pub fn new(request: &ProxyRequest, header_allowlist: &[String]) -> Self {
        let headers = redact::redact_headers(&request.headers, header_allowlist);
        Self {
            request_id: request.request_id.clone(),
            agent_id: request.agent_id.clone(),
            source_ip: request.source.ip().to_string(),
            method: request.method.as_str().to_string(),
            scheme: request.scheme.as_str().to_string(),
            host: request.host.clone(),
            port: request.port,
            path: redact::redact_path(&request.path),
            headers: serde_json::to_string(&headers).unwrap_or_default(),
            decision: "pending".to_string(),
            engine: String::new(),
            policy_ids: String::new(),
            deny_reason: String::new(),
            status: 0,
            bytes_in: 0,
            bytes_out: 0,
            latency_ms: 0.0,
            tls: TlsInfo::None.as_str().to_string(),
            error: String::new(),
        }
    }

    /// Record the policy decision.
    pub fn decision(mut self, decision: &PolicyDecision) -> Self {
        self.decision = decision.decision.as_str().to_string();
        self.engine = decision.engine.clone();
        self.policy_ids = decision.policy_ids.join(",");
        if !decision.is_allowed() {
            self.deny_reason = decision.reason.clone().unwrap_or_default();
        }
        self
    }

    /// Record TLS metadata.
    pub fn tls(mut self, tls: TlsInfo) -> Self {
        self.tls = tls.as_str().to_string();
        self
    }

    /// Record the final outcome of the request.
    pub fn outcome(mut self, status: u16, bytes_in: u64, bytes_out: u64, started: Instant) -> Self {
        self.status = status;
        self.bytes_in = bytes_in;
        self.bytes_out = bytes_out;
        self.latency_ms = started.elapsed().as_secs_f64() * 1000.0;
        self
    }

    /// Record a transport error message.
    pub fn error(mut self, error: impl Into<String>) -> Self {
        self.error = error.into();
        self
    }

    /// Emit the record as one structured `tracing` event (JSON via the
    /// configured subscriber).
    pub fn emit(self) {
        tracing::info!(
            target: "leash_proxy::audit",
            request_id = %self.request_id,
            agent_id = %self.agent_id,
            source_ip = %self.source_ip,
            method = %self.method,
            scheme = %self.scheme,
            host = %self.host,
            port = self.port,
            path = %self.path,
            headers = %self.headers,
            decision = %self.decision,
            engine = %self.engine,
            policy_ids = %self.policy_ids,
            deny_reason = %self.deny_reason,
            status = self.status,
            bytes_in = self.bytes_in,
            bytes_out = self.bytes_out,
            latency_ms = round_ms(self.latency_ms),
            tls = %self.tls,
            error = %self.error,
            "request"
        );
    }
}

fn round_ms(latency_ms: f64) -> f64 {
    (latency_ms * 1000.0).round() / 1000.0
}
