# Roadmap

Planned enhancements:

- Additional transparent networking mechanisms beyond Linux REDIRECT
  (HTTP/1.1 and HTTP/2 TLS interception with a supplied CA are supported).
- Exporters/sidecar channels for the Elixir collector (the JSONL log is
  already the durable source of truth).
- Idle timeouts for long-lived tunnels, and a policy dry-run (`--eval`) CLI
  mode.
