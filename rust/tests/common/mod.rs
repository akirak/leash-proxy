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
