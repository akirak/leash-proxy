# Configuration

CLI flags override a TOML file, which overrides defaults.

```toml
[server]
listen = "0.0.0.0:8080"

[agents]
# static source IP -> agent id mapping used for Cedar principals
"10.0.0.5" = "vm-alpha"

[policy]
schema = "policies/schema.cedar"
policies = ["policies/policies.cedar"]

[logging]
log_file = "var/leash-proxy.jsonl"   # stdout when absent
header_allowlist = ["host", "user-agent", "accept", "content-type", "content-length"]
```

## Keys

| Key | Description |
| --- | ----------- |
| `server.listen` | Address and port the proxy listens on. |
| `agents` | Static source IP → agent id mapping used for Cedar principals. |
| `policy.schema` | Path to the Cedar schema file. |
| `policy.policies` | Paths to the Cedar policy set files (Cedar text or Cedar JSON). |
| `logging.log_file` | JSONL log destination; stdout when absent. |
| `logging.header_allowlist` | Headers that may be recorded in audit logs. |

See [Cedar policies](/guide/policies) for how agents and requests map to
Cedar entities, and [Audit log](/guide/audit-log) for what gets logged.
