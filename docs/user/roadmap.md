# Roadmap

Planned enhancements:

- Additional transparent networking mechanisms beyond Linux REDIRECT
  (HTTP/1.1 and HTTP/2 TLS interception with a supplied CA are supported).
- Automatic disk spooling/replay and additional sidecar transports for the
  Elixir collector (bounded HTTP export and JSONL file tailing are supported).
- Idle timeouts for long-lived tunnels, and a policy dry-run (`--eval`) CLI
  mode.
