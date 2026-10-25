//! leash-proxy: a policy-enforcing egress proxy for AI agents.
//!
//! leash-proxy runs as the network gateway for AI agents inside micro VMs
//! (ADR 0001). Every proxied request — plain HTTP forwarding or CONNECT
//! tunneling — is evaluated against a Cedar policy engine with a fail-closed
//! posture before any bytes reach the outside world, and every request is
//! recorded as a structured, credential-hygienic audit event that can be
//! indexed downstream for monitoring (ADR 0002/0003).
//!
//! Main components:
//!
//! - [`proxy`]: the proxy API surface and connection handling
//!   ([`proxy::serve`], [`proxy::ProxyState`], [`proxy::ProxyRequest`]).
//! - [`policy`]: policy engines ([`policy::PolicyEngine`],
//!   [`policy::CedarEngine`], [`policy::EngineStack`]).
//! - [`audit`]: structured audit records ([`audit::AuditRecord`]).
//! - [`logging`]: JSONL `tracing` setup.
//! - [`redact`]: credential hygiene (path and header redaction).
//!
//! Exporters for downstream collection and TLS MITM termination are planned
//! as future enhancements.

pub mod audit;
pub mod cli;
pub mod config;
pub mod logging;
pub mod policy;
pub mod proxy;
pub mod redact;
