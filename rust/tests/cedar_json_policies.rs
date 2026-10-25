//! Cedar JSON (EST) policy format tests.
//!
//! Policy sources may be Cedar text or Cedar JSON; a file whose content starts
//! with `{` is parsed as JSON. These tests cover the JSON path of
//! `CedarEngine::from_files`/`from_sources`, including the policy set
//! generated from Lean 4 by `examples/lean-policy-gen` (mirrored by the
//! `JSON_POLICIES` fixture, which a dev-only test checks against the
//! checked-in generated file).

mod common;

use std::net::SocketAddr;
use std::path::PathBuf;

use http::{HeaderMap, Method, Version};
use leash_proxy::policy::{CedarEngine, PolicyEngine};
use leash_proxy::proxy::{ProxyRequest, Scheme};

use common::{JSON_POLICIES, SCHEMA};

/// The Cedar JSON policy file generated from Lean 4 by
/// `examples/lean-policy-gen` (`lake exe gen`).
const LEAN_GENERATED_FILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../examples/lean-policy-gen/policies.json"
);

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

/// Write policy sources to scratch files, one per entry, in a fresh temp
/// directory; returns the paths in order.
fn write_sources(sources: &[(&str, &str)]) -> Vec<PathBuf> {
    let dir = std::env::temp_dir().join(format!(
        "leash-json-policies-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    sources
        .iter()
        .enumerate()
        .map(|(i, (name, source))| {
            let path = dir.join(format!("{i}-{name}"));
            std::fs::write(&path, source).unwrap();
            path
        })
        .collect()
}

/// The `admin.internal` forbid must deny every agent, even the one that other
/// policies would permit, and record the forbid policy.
#[track_caller]
fn assert_admin_forbidden(engine: &CedarEngine) {
    let decision = engine.evaluate(&request("vm-build", Method::GET, "admin.internal", "/"));
    assert!(!decision.is_allowed(), "decision: {decision:?}");
    assert!(
        decision
            .reason
            .as_deref()
            .unwrap()
            .contains("explicitly forbidden"),
        "decision: {decision:?}"
    );
    assert!(!decision.policy_ids.is_empty());
}

#[test]
fn json_policy_set_loads_from_a_file_and_validates() {
    let schema_path = &write_sources(&[("schema.cedarschema", SCHEMA)])[0];
    let policy_paths = write_sources(&[("policies.json", JSON_POLICIES)]);
    let engine = CedarEngine::from_files(schema_path, &policy_paths);
    assert!(engine.is_ready());
}

/// The JSON policy set generated from Lean 4 mirrors the default Cedar text
/// policy set (`rust/policies/policies.cedar`): read-only GitHub API access
/// for every agent, PyPI `/simple/` access for the build agent, and a
/// blanket forbid of the internal admin console.
#[test]
fn lean_generated_json_policy_set_behaves_like_the_default_text_policy_set() {
    let schema_path = &write_sources(&[("schema.cedarschema", SCHEMA)])[0];
    let policy_paths = write_sources(&[("policies.json", JSON_POLICIES)]);
    let engine = CedarEngine::from_files(schema_path, &policy_paths);
    assert!(engine.is_ready());

    // Read-only GitHub API access is granted to every agent.
    assert!(
        engine
            .evaluate(&request(
                "vm-alpha",
                Method::GET,
                "api.github.com",
                "/repos/octocat"
            ))
            .is_allowed()
    );
    assert!(
        engine
            .evaluate(&request(
                "vm-build",
                Method::GET,
                "api.github.com",
                "/repos/octocat"
            ))
            .is_allowed()
    );

    // The build agent may install python packages from PyPI, but only from
    // the `/simple/` index.
    assert!(
        engine
            .evaluate(&request(
                "vm-build",
                Method::GET,
                "pypi.org",
                "/simple/numpy/"
            ))
            .is_allowed()
    );
    assert!(
        !engine
            .evaluate(&request(
                "vm-build",
                Method::GET,
                "pypi.org",
                "/packages/numpy/"
            ))
            .is_allowed()
    );
    assert!(
        !engine
            .evaluate(&request(
                "vm-build",
                Method::POST,
                "pypi.org",
                "/simple/numpy/"
            ))
            .is_allowed()
    );

    // Other agents get nothing, and other hosts nothing.
    assert!(
        !engine
            .evaluate(&request(
                "vm-alpha",
                Method::GET,
                "pypi.org",
                "/simple/numpy/"
            ))
            .is_allowed()
    );
    assert!(
        !engine
            .evaluate(&request(
                "vm-build",
                Method::GET,
                "files.pythonhosted.org",
                "/"
            ))
            .is_allowed()
    );

    assert_admin_forbidden(&engine);
}

/// Dev/repo check: the checked-in generated file (when present) must match
/// the `JSON_POLICIES` fixture, so the Lean example and the fixture cannot
/// drift apart. Skipped when the file is absent, e.g. in a source-filtered
/// build sandbox.
#[test]
fn lean_generated_file_matches_the_json_policies_fixture() {
    let path = PathBuf::from(LEAN_GENERATED_FILE);
    let Ok(on_disk) = std::fs::read_to_string(&path) else {
        return;
    };
    let expected: serde_json::Value = serde_json::from_str(JSON_POLICIES).unwrap();
    let actual: serde_json::Value = serde_json::from_str(&on_disk)
        .unwrap_or_else(|err| panic!("{path:?} is not valid JSON: {err}"));
    assert_eq!(
        expected,
        actual,
        "{} is stale: regenerate it with `lake exe gen` in examples/lean-policy-gen",
        path.display()
    );
}

#[test]
fn from_sources_accepts_a_json_policy_set_string() {
    let engine = CedarEngine::from_sources(SCHEMA, JSON_POLICIES);
    assert!(engine.is_ready());
    assert!(
        engine
            .evaluate(&request(
                "vm-build",
                Method::GET,
                "pypi.org",
                "/simple/numpy/"
            ))
            .is_allowed()
    );
    assert_admin_forbidden(&engine);
}

#[test]
fn mixed_text_and_json_files_are_one_policy_set() {
    // The Cedar text test policy set plus a JSON file that adds a pypi
    // permit the text set does not grant.
    let json = r#"{
        "templates": {},
        "staticPolicies": {
            "pypi": {
                "effect": "permit",
                "principal": {"op": "==", "entity": {"type": "Agent", "id": "vm-build"}},
                "action": {"op": "==", "entity": {"type": "Action", "id": "http_get"}},
                "resource": {"op": "==", "entity": {"type": "Host", "id": "pypi.org"}},
                "conditions": []
            }
        },
        "templateLinks": []
    }"#;
    let paths = write_sources(&[
        ("schema.cedarschema", SCHEMA),
        ("policies.cedar", common::POLICIES),
        ("pypi.json", json),
    ]);
    let engine = CedarEngine::from_files(&paths[0], &paths[1..]);
    assert!(engine.is_ready());

    // The Cedar text file is in effect.
    assert!(
        engine
            .evaluate(&request(
                "vm-alpha",
                Method::GET,
                "api.github.com",
                "/repos/octocat"
            ))
            .is_allowed()
    );
    // The JSON file is in effect in the same policy set.
    assert!(
        engine
            .evaluate(&request(
                "vm-build",
                Method::GET,
                "pypi.org",
                "/simple/numpy/"
            ))
            .is_allowed()
    );
    // Nothing grants pypi to other agents.
    assert!(
        !engine
            .evaluate(&request(
                "vm-alpha",
                Method::GET,
                "pypi.org",
                "/simple/numpy/"
            ))
            .is_allowed()
    );
    // The text file's forbid still applies to every agent.
    assert_admin_forbidden(&engine);
}

#[test]
fn json_policy_sets_with_templates_and_links_are_merged() {
    let json = r#"{
        "templates": {
            "read-template": {
                "effect": "permit",
                "principal": {"op": "==", "slot": "?principal"},
                "action": {"op": "==", "entity": {"type": "Action", "id": "http_get"}},
                "resource": {"op": "==", "entity": {"type": "Host", "id": "example.com"}},
                "conditions": []
            }
        },
        "staticPolicies": {},
        "templateLinks": [
            {
                "newId": "example-read",
                "templateId": "read-template",
                "values": {
                    "?principal": {"type": "Agent", "id": "vm-beta"}
                }
            }
        ]
    }"#;
    let paths = write_sources(&[("schema.cedarschema", SCHEMA), ("linked.json", json)]);
    let engine = CedarEngine::from_files(&paths[0], &paths[1..]);
    assert!(engine.is_ready());

    // The template-linked policy is live in the merged set.
    assert!(
        engine
            .evaluate(&request("vm-beta", Method::GET, "example.com", "/"))
            .is_allowed()
    );
    assert!(
        !engine
            .evaluate(&request("vm-alpha", Method::GET, "example.com", "/"))
            .is_allowed()
    );
}

#[test]
fn fails_closed_when_json_policies_are_malformed() {
    let engine = CedarEngine::from_sources(SCHEMA, "{not json");
    assert!(!engine.is_ready());
    assert!(
        !engine
            .evaluate(&request("vm-build", Method::GET, "pypi.org", "/"))
            .is_allowed()
    );
}

#[test]
fn fails_closed_when_json_policies_do_not_validate_against_schema() {
    // References an attribute that does not exist on Host.
    let json = r#"{
        "templates": {},
        "staticPolicies": {
            "bad": {
                "effect": "permit",
                "principal": {"op": "All"},
                "action": {"op": "All"},
                "resource": {"op": "All"},
                "conditions": [
                    {"kind": "when", "body": {".": {"left": {"Var": "resource"}, "attr": "nope"}}}
                ]
            }
        },
        "templateLinks": []
    }"#;
    let engine = CedarEngine::from_sources(SCHEMA, json);
    assert!(!engine.is_ready());
    assert!(
        !engine
            .evaluate(&request("vm-build", Method::GET, "pypi.org", "/"))
            .is_allowed()
    );
}

#[test]
fn fails_closed_when_json_policies_use_an_unknown_entity_type() {
    let json = r#"{
        "templates": {},
        "staticPolicies": {
            "bad": {
                "effect": "permit",
                "principal": {"op": "==", "entity": {"type": "User", "id": "alice"}},
                "action": {"op": "All"},
                "resource": {"op": "All"},
                "conditions": []
            }
        },
        "templateLinks": []
    }"#;
    let engine = CedarEngine::from_sources(SCHEMA, json);
    assert!(!engine.is_ready());
}
