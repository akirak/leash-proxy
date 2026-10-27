# Leash collector and audit console

A Phoenix 1.8 / LiveView application that indexes the Rust proxy's audit log
into SQLite. The Rust proxy remains independent: collector or database outages
never block policy evaluation or forwarding. No message broker is required.

```text
Rust proxy → append-only JSONL → file-tail Broadway producer
  → normalization → Broadway batcher → SQLite → PubSub → LiveView
```

## Run locally

Use the repository's Nix development environment (`nix develop`) or install
Elixir 1.17+ and a compatible Erlang/OTP version, plus a C toolchain for SQLite
and the test HTML parser.

From this directory:

```sh
mix setup
LEASH_AUDIT_LOG=/absolute/path/to/leash-proxy.jsonl mix phx.server
```

Run the Rust proxy separately with `--log-file` pointing to the **same file**.
See [proxy setup](../docs/user/guide/getting-started.md) for policy and listener
configuration. Paths may be absolute or relative to the collector's working
directory. It is safe to start the collector before the proxy creates the log.

Visit <http://localhost:4000>:

- `/`: 24-hour request counts and top denied hosts.
- `/events`: live audit feed, decision/agent/host/path/time filters, pagination,
  and pause/resume. Filters and page numbers are bookmarkable URL parameters.
- `/events/:request_id`: request identity, redacted headers, policy decisions,
  outcome, transfer statistics, and TLS metadata.

The UI is read-only; policy authoring and proxy control are not implemented.
There is **no authentication yet**. Keep the UI on a trusted local interface,
SSH tunnel, or behind an authenticated reverse proxy. Audit metadata can be
sensitive even though the proxy redacts request data.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `LEASH_AUDIT_LOG` | `tmp/audit.jsonl` | Proxy's append-only JSONL log |
| `LEASH_AUDIT_CURSOR` | `.<log filename>.cursor` beside log | Collector checkpoint; must be writable |
| `LEASH_EXPORTER_TOKEN` | unset (HTTP ingestion disabled) | Shared bearer token for `/api/audit/events` |
| `PORT` | `4000` | Web listener port |
| `DATABASE_PATH` | required in production | SQLite database path |
| `SECRET_KEY_BASE` | required in production | Generate with `mix phx.gen.secret` |
| `PHX_HOST` | `example.com` in production | Public hostname |
| `PHX_SERVER` | unset | Set to `true` to serve HTTP in a release |
| `POOL_SIZE` | `5` in production | Ecto connection pool size |

Development stores data in `leash_collector_dev.db`. Tests use a separate,
partition-aware database and disable the application-level ingestion pipeline.
Releases migrate the database on startup. Production binds on all interfaces by
default; restrict network access or configure the endpoint before deployment.

## HTTP push ingestion

The Rust proxy can optionally push batches to `POST /api/audit/events` without
sharing a filesystem. Set the same nonempty `LEASH_EXPORTER_TOKEN` in both
processes, and configure `--exporter-url` on the proxy. See
[exporter configuration](../docs/user/guide/configuration.md#phoenix-http-exporter).
Use HTTPS for remote connections; plain HTTP is only suitable on a trusted
local network. The existing browser UI remains unauthenticated.

Requests must supply `Authorization: Bearer <token>` and a JSON body
`{"events": [event, ...]}` containing 1–200 flattened `leash_proxy::audit`
records. The endpoint normalizes records using the existing decoder, rejects
an entire malformed batch with HTTP 422, and commits directly through the
idempotent batch sink before responding `200 {"inserted": N}`. Database
failures return non-success responses so the proxy can retry. Unconfigured
ingestion returns 503; missing/invalid authorization returns 401. Validation
errors do not echo tokens or raw invalid events.

HTTP push and Broadway file tailing can run together; duplicate request IDs
are ignored. Push delivery is best effort, with a bounded in-memory queue and
limited retries. Keep the proxy's JSONL file for recovery; no disk spool or
automatic HTTP replay is implemented. File tailing below remains the durable
replay mechanism.

## Delivery and replay

- The tail producer consumes only `leash_proxy::audit` events, respects demand,
  and allows only one read chunk in flight. It waits for complete lines and
  polls every 500 ms. Normal chunks are 512 KB; the read window can grow to
  accommodate a line, up to a 16 MiB limit. Oversized lines are skipped through
  their next newline, with a warning.
- Broadway uses two processors and one SQLite batch writer, batching up to
  200 events or 1 second. The sink uses `Repo.insert_all/3`, not per-row inserts.
- The cursor advances **after all messages in a chunk are acknowledged**.
  Failed chunks are retried. Cursor files are replaced atomically. A crash
  between insertion and checkpointing can replay events; the unique
  `request_id` index makes inserts idempotent.
- Operational lines are ignored. Invalid JSON and malformed audit records
  are skipped with warnings, without logging their raw contents. Required
  fields and numeric types/ranges are validated before reaching SQLite.
- File replacement or detected truncation restarts the current file at zero.
  This is not a spool-directory reader: it does not finish unread content in
  renamed files. Drain the collector before rotating, retain original logs,
  and avoid copy-truncate rotation (a truncate/regrow between polls cannot
  reliably be detected). Run one collector per source/cursor; do not delete
  logs before they have been drained.

SQLite is a rebuildable index; retain the original JSONL files separately.
To replay, stop the collector, remove its cursor, and restart against a retained
log. Existing rows are deduplicated. If rebuilding an empty database, **also
reset the cursor**, or already-checkpointed records will not be re-indexed.
`LeashCollector.Audit.prune_before/1` is available for explicit index retention;
there is no automatic retention job yet.

## Future broker transport

The decoder, batch sink, query context, and LiveViews are independent of file
tailing. A Broadway-compatible producer can replace it through application
configuration without changing those components:

```elixir
# After adding a broker integration dependency and its connection settings:
config :leash_collector, :audit_pipeline,
  producer: [module: {YourBroker.Producer, broker_options}, concurrency: 1]
```

Broker messages may contain decoded maps or JSON binaries matching the proxy's
flattened audit-event format (`rust/src/audit.rs`). The pipeline returns failed
acknowledgements for database failures, so configure the broker producer's
retry/requeue behavior. Malformed events are intentionally acknowledged and
skipped. At-least-once broker delivery is supported by the same request-ID
uniqueness constraint. No broker dependency or service is installed today.

## Development checks

```sh
mix test
mix precommit
mix assets.build
```

Tests cover normalization, batching and idempotency, database-failure
acknowledgements, cursor acknowledgement/restart behavior, truncation/rotation,
partial and oversized lines, broker-style producer replacement, query windows,
literal substring filters, and LiveView filtering/pagination/live updates.
