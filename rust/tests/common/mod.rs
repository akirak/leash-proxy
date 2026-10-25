//! Shared Cedar test fixtures for the integration tests.

pub const SCHEMA: &str = r#"
entity Agent;

entity Host {
    port?: Long,
    scheme?: String,
};

action
    "http_connect",
    "http_delete",
    "http_get",
    "http_head",
    "http_options",
    "http_patch",
    "http_post",
    "http_put",
    "http_trace"
    appliesTo {
        principal: Agent,
        resource: Host,
        context: {
            path: String,
        },
    };
"#;

pub const POLICIES: &str = r#"
// vm-alpha may read from api.github.com.
permit(principal == Agent::"vm-alpha", action == Action::"http_get", resource == Host::"api.github.com");

// vm-alpha may read from the loopback test upstream.
permit(principal == Agent::"vm-alpha", action == Action::"http_get", resource == Host::"127.0.0.1");

// Any agent may tunnel to the loopback test upstream.
permit(principal, action == Action::"http_connect", resource == Host::"127.0.0.1");

// Agents may not POST anywhere, to prove deny-by-default.
// (no permit for http_post at all)

// The internal admin console is always forbidden.
forbid(principal, action, resource == Host::"admin.internal");
"#;

/// A Cedar policy set in the Cedar JSON (EST) format: the policy set that
/// `examples/lean-policy-gen` generates from Lean 4 (`lake exe gen`). It is
/// equivalent to the default Cedar text policy set in `rust/policies/`
/// (`policies.cedar`). A dev-only test asserts that the checked-in generated
/// file matches this fixture.
#[allow(dead_code)] // only some test targets use the JSON fixtures
pub const JSON_POLICIES: &str = r#"{
  "staticPolicies": {
    "github-readonly": {
      "effect": "permit",
      "principal": {"op": "All"},
      "action": {"op": "==", "entity": {"type": "Action", "id": "http_get"}},
      "resource": {"op": "==", "entity": {"type": "Host", "id": "api.github.com"}},
      "conditions": []
    },
    "pypi-simple": {
      "effect": "permit",
      "principal": {"op": "==", "entity": {"type": "Agent", "id": "vm-build"}},
      "action": {"op": "==", "entity": {"type": "Action", "id": "http_get"}},
      "resource": {"op": "==", "entity": {"type": "Host", "id": "pypi.org"}},
      "conditions": [
        {
          "kind": "when",
          "body": {
            "like": {
              "left": {".": {"left": {"Var": "context"}, "attr": "path"}},
              "pattern": [
                {"Literal": "/"},
                {"Literal": "s"},
                {"Literal": "i"},
                {"Literal": "m"},
                {"Literal": "p"},
                {"Literal": "l"},
                {"Literal": "e"},
                {"Literal": "/"},
                "Wildcard"
              ]
            }
          }
        }
      ]
    },
    "forbid-admin": {
      "effect": "forbid",
      "principal": {"op": "All"},
      "action": {"op": "All"},
      "resource": {"op": "==", "entity": {"type": "Host", "id": "admin.internal"}},
      "conditions": []
    }
  },
  "templates": {},
  "templateLinks": []
}"#;
