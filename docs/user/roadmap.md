# Roadmap

Planned enhancements:

- TLS MITM termination with a per-host certificate minted from a CA key (the
  request/response and audit types already carry scheme and TLS metadata).
- Exporters/sidecar channels for the Elixir collector (the JSONL log is
  already the durable source of truth).
- Idle timeouts for long-lived tunnels, and a policy dry-run (`--eval`) CLI
  mode.
