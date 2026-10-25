# Architecture

```text
micro VM guest ──HTTP/CONNECT──► leash-proxy (Rust)
                                   ├─ policy engines (Cedar, fail-closed)
                                   └─ audit records (tracing, JSONL) ──► future exporters/collector
```

## Rust component

The proxy lives in [`rust/`](https://github.com/akirakomamura/leash-proxy/tree/main/rust)
as a library (`leash_proxy`) plus a thin binary (`leash-proxy`).

- `proxy` — the proxy API surface: `ProxyRequest`, `ProxyState`, `serve`, plus
  HTTP/1.1 forward-proxying (absolute-form requests), CONNECT tunneling, an
  `AgentResolver` for micro VM identification, and a counting body that tracks
  `bytes_in`/`bytes_out` without buffering.
- `policy` — policy engines: the `PolicyEngine` trait, a Cedar engine
  (`CedarEngine`) that loads and validates a schema and policy set up front,
  and an `EngineStack` that composes engines with deny-wins semantics. Any
  load/validation failure is fail-closed: every request is denied.
- `audit` — one structured record per request (request id, agent, method,
  scheme, host, redacted path, allowlisted headers, decision, determining
  policy IDs, deny reason, status, bytes, latency, TLS metadata).
- `redact` — credential hygiene: query strings are stripped from logged paths
  and only allowlisted headers are ever recorded.
- `logging` — JSONL `tracing` output to stdout or an append-only file; audit
  events always flow at INFO regardless of `RUST_LOG`.

Downstream, the JSONL log is the durable source of truth (ADR 0002); the
Elixir/Phoenix collector and monitoring UI run as a separate process (ADR 0003).

## Next steps

- [Getting started](/guide/getting-started) — build and run the proxy.
- [Configuration](/guide/configuration) — CLI flags, TOML file, and defaults.
- [Cedar policies](/guide/policies) — the policy model and examples.
