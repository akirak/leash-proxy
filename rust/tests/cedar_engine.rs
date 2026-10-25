//! Cedar policy engine tests.

mod common;

use http::{HeaderMap, Method, Version};
use std::net::SocketAddr;

use leash_proxy::policy::{CedarEngine, PolicyEngine};
use leash_proxy::proxy::{ProxyRequest, Scheme};

use common::{POLICIES, SCHEMA};

fn request(agent: &str, method: Method, host: &str, path: &str) -> ProxyRequest {
    ProxyRequest {
        request_id: "test-request".to_string(),
        agent_id: agent.to_string(),
        source: "10.0.0.5:40000".parse::<SocketAddr>().unwrap(),
        method,
        scheme: Scheme::Http,
        host: host.to_string(),
        port: 80,
        path: path.to_string(),
        version: Version::HTTP_11,
        headers: HeaderMap::new(),
    }
}

#[test]
fn engine_loads_and_is_ready() {
    let engine = CedarEngine::from_sources(SCHEMA, POLICIES);
    assert!(engine.is_ready());
}

#[test]
fn allows_permitted_request() {
    let engine = CedarEngine::from_sources(SCHEMA, POLICIES);
    let decision = engine.evaluate(&request(
        "vm-alpha",
        Method::GET,
        "api.github.com",
        "/repos/octocat",
    ));
    assert!(decision.is_allowed(), "decision: {decision:?}");
    assert_eq!(decision.engine, "cedar");
    assert!(
        !decision.policy_ids.is_empty(),
        "determining policy id recorded"
    );
}

#[test]
fn denies_by_default_for_unmatched_requests() {
    let engine = CedarEngine::from_sources(SCHEMA, POLICIES);
    let decision = engine.evaluate(&request("vm-beta", Method::GET, "api.github.com", "/"));
    assert!(!decision.is_allowed());
    assert!(
        decision
            .reason
            .as_deref()
            .unwrap()
            .contains("no policy permits")
    );
}

#[test]
fn denies_unmapped_agent_by_default() {
    let engine = CedarEngine::from_sources(SCHEMA, POLICIES);
    let decision = engine.evaluate(&request("ip:10.0.0.9", Method::GET, "api.github.com", "/"));
    assert!(!decision.is_allowed());
}

#[test]
fn explicit_forbid_wins() {
    let engine = CedarEngine::from_sources(SCHEMA, POLICIES);
    // First show the path would otherwise be grantable, then that the forbid
    // denies it.
    let allowed = engine.evaluate(&request("vm-alpha", Method::GET, "127.0.0.1", "/"));
    assert!(allowed.is_allowed());
    let forbidden = engine.evaluate(&request("vm-alpha", Method::GET, "admin.internal", "/"));
    assert!(!forbidden.is_allowed());
    assert!(
        forbidden
            .reason
            .as_deref()
            .unwrap()
            .contains("explicitly forbidden"),
        "decision: {forbidden:?}"
    );
    assert!(
        !forbidden.policy_ids.is_empty(),
        "forbid policy id recorded"
    );
}

#[test]
fn connect_tunnel_is_its_own_action() {
    let engine = CedarEngine::from_sources(SCHEMA, POLICIES);
    let mut tunnel = request("vm-alpha", Method::CONNECT, "127.0.0.1", "");
    tunnel.scheme = Scheme::Tunnel;
    tunnel.port = 8080;
    let decision = engine.evaluate(&tunnel);
    assert!(decision.is_allowed(), "decision: {decision:?}");
}

#[test]
fn fails_closed_when_schema_is_invalid() {
    let engine = CedarEngine::from_sources("this is not a schema", POLICIES);
    assert!(!engine.is_ready());
    let decision = engine.evaluate(&request("vm-alpha", Method::GET, "api.github.com", "/"));
    assert!(!decision.is_allowed());
    assert!(
        decision
            .reason
            .as_deref()
            .unwrap()
            .contains("failing closed")
    );
}

#[test]
fn fails_closed_when_policies_are_invalid() {
    let engine = CedarEngine::from_sources(SCHEMA, "this is not a policy");
    assert!(!engine.is_ready());
    let decision = engine.evaluate(&request("vm-alpha", Method::GET, "api.github.com", "/"));
    assert!(!decision.is_allowed());
}

#[test]
fn fails_closed_when_policies_do_not_validate_against_schema() {
    // The policy references an attribute that does not exist on Host.
    let invalid = r#"permit(principal, action, resource) when { resource.nope }; "#;
    let engine = CedarEngine::from_sources(SCHEMA, invalid);
    assert!(!engine.is_ready());
    let decision = engine.evaluate(&request("vm-alpha", Method::GET, "api.github.com", "/"));
    assert!(!decision.is_allowed());
}

#[test]
fn fails_closed_for_unknown_methods() {
    let engine = CedarEngine::from_sources(SCHEMA, POLICIES);
    let decision = engine.evaluate(&request(
        "vm-alpha",
        Method::from_bytes(b"BREW").unwrap(),
        "api.github.com",
        "/",
    ));
    // The unknown method maps to a Cedar action that the schema does not
    // declare; the request must still be denied with a diagnostic reason.
    assert!(!decision.is_allowed());
    assert!(decision.reason.is_some());
    let reason = decision.reason.unwrap();
    assert!(
        reason.contains("cedar") || reason.contains("http_brew"),
        "reason should point at the unknown action: {reason}"
    );
}
