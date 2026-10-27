# Audit log

Every request is recorded as one JSON line (target `leash_proxy::audit`) in
the file configured under `logging.log_file`, or on stdout.

```json
{"timestamp":"…","level":"INFO","message":"request","request_id":"…","agent_id":"vm-alpha","source_ip":"10.0.0.5","method":"GET","scheme":"http","host":"api.github.com","port":80,"path":"/repos/octocat","headers":"{\"host\":\"api.github.com\"}","decision":"deny","engine":"cedar","policy_ids":"","deny_reason":"no policy permits this request","status":403,"bytes_in":0,"bytes_out":0,"latency_ms":0.604,"tls":"none","error":"","target":"leash_proxy::audit"}
```

## Fields

| Field | Description |
| ----- | ----------- |
| `request_id` | Unique id assigned to the request. |
| `agent_id`, `source_ip` | Resolved agent and guest source IP. |
| `method`, `scheme`, `host`, `port`, `path` | Request target. The path is redacted: query strings are stripped. |
| `headers` | Only allowlisted headers are recorded. |
| `decision`, `engine`, `policy_ids`, `deny_reason` | Authorization outcome and the policies that determined it. |
| `status` | Response status returned to the guest. |
| `bytes_in`, `bytes_out` | Body bytes observed in each direction, without buffering. |
| `latency_ms` | Round-trip duration. |
| `tls` | TLS metadata (e.g. `none` for plain HTTP). |

The JSONL log is the source of truth (ADR 0002); downstream indexing feeds
the monitoring UI (ADR 0003).

## Phoenix export delivery

The optional [Phoenix HTTP exporter](/guide/configuration#phoenix-http-exporter)
receives only `leash_proxy::audit` events, independent of `RUST_LOG`, in the
same flattened JSON format as the file log (including timestamps and existing
redaction). Operational logs are not exported. The file sink is unchanged.

Export is **best effort**, not a durable queue: request processing never waits
for the collector. A bounded in-memory queue drops new export copies when full.
Events larger than 64 KiB are omitted from export, but still written to the
normal log. Batches contain at most 200 events and approximately 512 KiB;
partial batches flush on the configured interval. Queue capacity is measured
in events, each capped at 64 KiB.

Network errors, request timeouts, HTTP 429, and HTTP 5xx retry the identical
batch with bounded exponential backoff (100 ms initially, capped at 6.4 s).
Other non-success responses, including redirects and authentication failures,
are not retried. After retries are exhausted, the exporter discards the batch
and continues. Dropped-event warnings contain counts, not event contents.
On shutdown the exporter closes its queue and drains accepted events, up to
`shutdown_timeout_ms`; remaining export copies are then discarded. Requests
that finish after the exporter closes are not guaranteed to be exported.

Retain the JSONL file using `--log-file` or `logging.log_file` (or capture
stdout durably). There is no automatic disk spool or HTTP replay. Recover
missing UI records by having the collector tail the retained JSONL file,
resetting its cursor if necessary; duplicate request IDs are ignored. The
collector's SQLite database is a rebuildable index, not the audit source
of truth.
