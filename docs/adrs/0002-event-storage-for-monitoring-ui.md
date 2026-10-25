# Keep Structured JSONL Logs as the Source of Truth and Query a Relational Database for the Monitoring UI

Date: 2026-10-25

## Status

Draft

## Context and Problem Statement

The MITM proxy logs individual HTTP/HTTPS requests and Cedar policy decisions as structured events.
We want to build a monitoring UI that supports live updates, filtering, drill-down into individual requests (e.g., all denied requests to `*.github.com` from a specific VM in the last hour), pagination, sorting, and aggregations (such as top denied hosts and per-agent request counts).
How should request logs be stored and queried to power the monitoring UI without degrading the proxy's request-processing hot path?

## Considered Options

* Keep structured JSONL logs as the source of truth and asynchronously index events into a relational database (SQLite or PostgreSQL) for the UI
* Have the monitoring UI parse the JSONL log files directly
* Store request logs in a dedicated time-series database (e.g., Prometheus, InfluxDB)

## Decision Outcome

Chosen option: "Keep structured JSONL logs as the source of truth and asynchronously index events into a relational database (SQLite or PostgreSQL) for the UI", because proxy request records are high-cardinality discrete events that require ad-hoc filtering, pagination, and point inspection while keeping the durable audit trail decoupled from database availability.

### Architecture and Schema

```text
proxy ──(tracing, JSON)──► append-only JSONL file / stdout   ← durable audit record
  │
  └─(async export / collector)──► SQLite (WAL) / PostgreSQL  ← UI queries this
```

* **Hot-path isolation:** The proxy must never block allow/deny decisions or request forwarding on database writes. Events are either tailed from the JSONL file by a collector or sent over a bounded asynchronous channel with explicit backpressure behavior (dropping or spooling UI export events if full, while never dropping the audit file log).
* **Schema and indexing:** Store `ts`, `request_id`, `agent_id`, `method`, `scheme`, `host`, `path` (with query redacted), `decision`, `policy_ids`, `deny_reason`, `status`, `bytes_in`, `bytes_out`, `latency_ms`, and `tls_info`, with indexes on `ts`, `host`, `decision`, and `agent_id`.
* **Retention and replayability:** Because the JSONL log is the source of truth, the database can be rebuilt from logs at any time and pruned via periodic `DELETE WHERE ts < ...` jobs or time-partitioned database files.
* **Graduation path:** SQLite (in WAL mode) or PostgreSQL is sufficient for a single host or small fleet; if deployment scales to many proxy hosts or millions of events per day, the collector sink can migrate to ClickHouse, Loki, or an OpenTelemetry backend.

### Consequences

* Good, because the UI can perform fast indexed queries, pagination, sorting, and aggregations without re-scanning log files.
* Good, because slow or locked database operations cannot add latency to proxied traffic or affect policy enforcement.
* Good, because maintaining an append-only JSONL file (or off-host log stream) preserves a durable, replayable audit trail.
* Bad, because operating both an append-only log and a database introduces an ingestion pipeline and storage retention lifecycle to manage.

## Pros and Cons of the Options

### Parse JSONL Log Files Directly in the UI

* Good, because it requires no additional storage component for a basic prototype or `tail -f` view.
* Bad, because filtering, pagination, sorting, and aggregations require repeated file scans that degrade quickly as log volume grows.
* Bad, because log rotation and retention policies can easily break UI state and queries.

### Dedicated Time-Series Database (Prometheus, InfluxDB)

* Good, because it excels at low-cardinality numeric metrics such as allow/deny rates, byte counters, and latency histograms (which can still be exported separately via Prometheus or OpenTelemetry).
* Bad, because request logs are high-cardinality events (`url`, `host`, `path`, `request_id`, `agent_id`, `policy_id`) and TSDBs perform poorly when indexing and inspecting individual event records.

## Related ADRs

* [0001-rust-mitm-proxy-with-cedar-policies.md](0001-rust-mitm-proxy-with-cedar-policies.md)
* [0003-elixir-phoenix-downstream-collector-and-ui.md](0003-elixir-phoenix-downstream-collector-and-ui.md)
