//! Policy engines.
//!
//! The central abstraction is the [`PolicyEngine`] trait. Every proxied
//! request is evaluated against an engine before any bytes are forwarded.
//! The [`EngineStack`] combines engines with deny-wins semantics, and the
//! [`CedarEngine`] evaluates Cedar policies with a fail-closed posture
//! (ADR 0001).

pub mod cedar_engine;
pub mod stack;

pub use cedar_engine::CedarEngine;
pub use stack::EngineStack;

use crate::proxy::ProxyRequest;

/// A policy decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
}

impl Decision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
        }
    }
}

/// The outcome of evaluating a request against a policy engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDecision {
    pub decision: Decision,
    /// Name of the engine that produced the decision.
    pub engine: String,
    /// IDs of the policies that determined the decision.
    pub policy_ids: Vec<String>,
    /// Human-readable reason for a deny, or diagnostics for an allow.
    pub reason: Option<String>,
}

impl PolicyDecision {
    pub fn allow(engine: impl Into<String>, policy_ids: Vec<String>) -> Self {
        Self {
            decision: Decision::Allow,
            engine: engine.into(),
            policy_ids,
            reason: None,
        }
    }

    pub fn deny(engine: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            decision: Decision::Deny,
            engine: engine.into(),
            policy_ids: Vec::new(),
            reason: Some(reason.into()),
        }
    }

    /// A deny that was determined by specific policies (e.g. a Cedar `forbid`).
    pub fn deny_by(
        engine: impl Into<String>,
        policy_ids: Vec<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            decision: Decision::Deny,
            engine: engine.into(),
            policy_ids,
            reason: Some(reason.into()),
        }
    }

    pub fn is_allowed(&self) -> bool {
        self.decision == Decision::Allow
    }
}

/// A pluggable policy enforcement point.
///
/// Implementations must be cheap to call from the request hot path and must
/// never block on external services: if a decision cannot be computed, the
/// engine must deny (fail closed).
pub trait PolicyEngine: Send + Sync {
    /// Short, stable engine name used in audit records.
    fn name(&self) -> &'static str;

    /// Evaluate a request and return the decision.
    fn evaluate(&self, request: &ProxyRequest) -> PolicyDecision;
}
