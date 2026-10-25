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
