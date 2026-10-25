# Use Rust with mitmproxy_rs and Cedar for the Agent MITM Proxy

Date: 2026-10-25

## Status

Draft

## Context and Problem Statement

AI agents running inside micro VMs require controlled outbound network access so they can only make allowed HTTP/HTTPS request patterns while all unallowed destinations are blocked.
Because inspecting HTTPS request patterns (such as method, path, and headers) requires TLS termination, the proxy must operate as a man-in-the-middle (MITM) proxy and hold a CA private key.
Every proxied request and policy decision must also be recorded in structured logs for auditing, debugging, and policy tuning without leaking sensitive credentials.

## Considered Options

* Implement the MITM proxy in Rust using `mitmproxy_rs` and `cedar-policy/cedar`
* Implement a network-layer firewall (e.g., iptables/nftables or DNS filtering) without TLS termination
* Embed or script an existing general-purpose proxy with custom policy hooks

## Decision Outcome

Chosen option: "Implement the MITM proxy in Rust using `mitmproxy_rs` and `cedar-policy/cedar`", because it provides memory-safe, low-overhead HTTP/HTTPS interception alongside expressive, formally verified policy evaluation and detailed decision diagnostics.

### Implementation Details

* **Traffic interception:** Use crates from `mitmproxy/mitmproxy_rs` to handle HTTP/HTTPS MITM proxying and TLS termination for traffic originating from micro VMs.
* **Policy evaluation:** Define authorization policies in Cedar and evaluate each request using `cedar-policy/cedar`.
* **Fail-closed posture:** If the proxy cannot load or validate its Cedar policies or schema, it must deny all requests.
* **Structured audit logging:** Emit a structured JSON log record for every request containing the timestamp, request ID, VM/agent ID, HTTP method, scheme, host, redacted path, Cedar decision, determining policy IDs, deny reason/diagnostics, response status, bytes in/out, latency, and TLS metadata.
* **Credential hygiene:** Because TLS termination exposes plaintext traffic, log request headers using an explicit allowlist rather than a denylist, omit request and response bodies by default, and strip query strings or tokens from logged URLs.

### Consequences

* Good, because Rust provides a small, auditable, memory-safe binary suitable for isolating the CA private key and policy enforcement point.
* Good, because Cedar policies produce explicit determining policy IDs and diagnostics, making it clear why a request was allowed or blocked.
* Bad, because terminating TLS makes the proxy a high-value security boundary that requires strict privilege separation and log redaction to prevent credential leakage.

## Related ADRs

* [0002-event-storage-for-monitoring-ui.md](0002-event-storage-for-monitoring-ui.md)
* [0003-elixir-phoenix-downstream-collector-and-ui.md](0003-elixir-phoenix-downstream-collector-and-ui.md)
