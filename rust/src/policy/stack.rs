//! Deny-wins composition of policy engines.

use std::sync::Arc;

use super::{PolicyDecision, PolicyEngine};
use crate::proxy::ProxyRequest;

/// A stack of [`PolicyEngine`]s evaluated in order.
///
/// A request is allowed only if *every* engine allows it; the first deny
/// short-circuits the stack and is returned as the decision. This lets a
/// deployment compose, for example, a baseline Cedar policy set with a
/// second engine enforcing additional rules, without any single engine being
/// able to grant access the others forbid.
pub struct EngineStack {
    engines: Vec<Arc<dyn PolicyEngine>>,
}

impl EngineStack {
    /// Create a stack from engines, evaluated in the given order.
    pub fn new(engines: Vec<Arc<dyn PolicyEngine>>) -> Self {
        Self { engines }
    }

    /// The engines in this stack, in evaluation order.
    pub fn engines(&self) -> &[Arc<dyn PolicyEngine>] {
        &self.engines
    }
}

impl PolicyEngine for EngineStack {
    fn name(&self) -> &'static str {
        "stack"
    }

    fn evaluate(&self, request: &ProxyRequest) -> PolicyDecision {
        let mut policy_ids = Vec::new();
        for engine in &self.engines {
            let decision = engine.evaluate(request);
            if !decision.is_allowed() {
                tracing::info!(
                    target: "leash_proxy::policy",
                    request_id = %request.request_id,
                    engine = %decision.engine,
                    "engine denied request; stack deny"
                );
                return decision;
            }
            policy_ids.extend(decision.policy_ids);
        }
        PolicyDecision::allow(self.name(), policy_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{Decision, PolicyDecision};
    use http::{HeaderMap, Method, Version};
    use std::net::SocketAddr;

    struct Stub {
        name: &'static str,
        outcome: Decision,
    }

    impl PolicyEngine for Stub {
        fn name(&self) -> &'static str {
            self.name
        }
        fn evaluate(&self, _request: &ProxyRequest) -> PolicyDecision {
            match self.outcome {
                Decision::Allow => PolicyDecision::allow(self.name, vec!["p1".into()]),
                Decision::Deny => PolicyDecision::deny(self.name, "stub deny"),
            }
        }
    }

    fn sample_request() -> ProxyRequest {
        ProxyRequest {
            request_id: "test".into(),
            agent_id: "vm-alpha".into(),
            source: "127.0.0.1:40000".parse::<SocketAddr>().unwrap(),
            method: Method::GET,
            scheme: crate::proxy::Scheme::Http,
            host: "example.com".into(),
            port: 80,
            path: "/".into(),
            version: Version::HTTP_11,
            headers: HeaderMap::new(),
        }
    }

    #[test]
    fn all_engines_must_allow() {
        let stack = EngineStack::new(vec![
            Arc::new(Stub {
                name: "a",
                outcome: Decision::Allow,
            }),
            Arc::new(Stub {
                name: "b",
                outcome: Decision::Allow,
            }),
        ]);
        let decision = stack.evaluate(&sample_request());
        assert!(decision.is_allowed());
        assert_eq!(
            decision.policy_ids,
            vec!["p1".to_string(), "p1".to_string()]
        );
    }

    #[test]
    fn first_deny_wins() {
        let stack = EngineStack::new(vec![
            Arc::new(Stub {
                name: "a",
                outcome: Decision::Allow,
            }),
            Arc::new(Stub {
                name: "b",
                outcome: Decision::Deny,
            }),
            Arc::new(Stub {
                name: "c",
                outcome: Decision::Allow,
            }),
        ]);
        let decision = stack.evaluate(&sample_request());
        assert!(!decision.is_allowed());
        assert_eq!(decision.engine, "b");
        assert_eq!(decision.reason.as_deref(), Some("stub deny"));
    }
}
