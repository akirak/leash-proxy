# Run the Elixir/Phoenix Collector and UI as a Separate Downstream Process Instead of Embedding Rust via FFI

Date: 2026-10-25

## Status

Draft

## Context and Problem Statement

We plan to build the log collector, policy management workflows, and live-updating monitoring UI using Elixir and Phoenix while keeping the MITM proxy and Cedar policy engine in Rust.
We need to decide whether the Elixir/Phoenix application should run as a separate downstream consumer of a standalone Rust proxy process, or whether the entire service should be hosted inside the BEAM VM by embedding the Rust proxy and policy engine via FFI (Rustler NIFs).

## Considered Options

* Run the Rust proxy as a standalone process and the Elixir/Phoenix collector and UI as a separate downstream sink
* Implement the entire service in Elixir and embed the Rust proxy and Cedar engine in the BEAM via FFI (NIFs/Rustler)

## Decision Outcome

Chosen option: "Run the Rust proxy as a standalone process and the Elixir/Phoenix collector and UI as a separate downstream sink", because it preserves fault isolation, runtime simplicity, availability independence, and a minimal security boundary around the CA private key and policy enforcement point.

### Architecture and Integration Boundaries

```text
Rust proxy (policy enforcement, CA key, Cedar)
   ├─► append-only JSONL / local spool   ← durable audit record
   └─► export channel ─► Elixir collector ─► Postgres/SQLite ─► Phoenix LiveView UI
```

* **One-way data plane:** Telemetry flows strictly from the Rust proxy to the Elixir collector. The Rust proxy must never depend on the Elixir application being reachable or healthy to evaluate policies or forward traffic.
* **Event transport:** Start with Elixir tailing the append-only JSONL file (or spool directory) on the same host for cursor-based replayability, or push events from a background Rust task over a Unix domain socket / HTTP / gRPC with a local disk buffer and idempotent inserts (`ON CONFLICT DO NOTHING`) if deployed across hosts.
* **Elixir responsibilities:** Use Broadway or GenStage for backpressured ingestion and batched database inserts, Phoenix LiveView and PubSub for live-updating UI feeds of denied requests, and Phoenix workflows for browsing and authoring Cedar policies.
* **Control plane separation:** If the UI pushes policy updates to the proxy, use a separate, authenticated control channel (such as Git/file-watched signed policy bundles or a narrow mTLS/local-socket admin API that validates the Cedar schema and records a versioned audit trail).
* **Policy dry-runs ("what-if" analysis):** To evaluate proposed Cedar policies against historical requests from Elixir, invoke a small Rust sidecar or run the proxy binary in a dedicated `--eval` CLI mode rather than embedding the proxy in the BEAM.

### Consequences

* Good, because crashes, deployments, or database migrations in the Elixir/Phoenix application cannot interrupt agent traffic or policy enforcement.
* Good, because the CA private key and TLS termination remain isolated in a small, minimal-privilege Rust binary rather than sharing a process with a web server and database stack.
* Good, because each runtime (Tokio in Rust, BEAM in Elixir) manages its own scheduling and lifecycle without cross-runtime contention.
* Bad, because deploying and coordinating two separate processes requires an inter-process transport protocol for log ingestion and policy reloads.

## Pros and Cons of the Options

### Embed the Rust Proxy in Elixir via FFI (NIFs/Rustler)

* Good, because the system can be packaged and deployed as a single OS process.
* Bad, because a panic, deadlock, or long-running network operation in a NIF can stall or crash the entire BEAM VM.
* Bad, because running an asynchronous Tokio runtime and thread pool inside the BEAM creates complex lifecycle and shutdown behavior.
* Bad, because embedding the proxy into the Phoenix web application widens the attack surface of the component holding the MITM CA private key.

## Related ADRs

* [0001-rust-mitm-proxy-with-cedar-policies.md](0001-rust-mitm-proxy-with-cedar-policies.md)
* [0002-event-storage-for-monitoring-ui.md](0002-event-storage-for-monitoring-ui.md)
