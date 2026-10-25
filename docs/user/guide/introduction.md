# Introduction

leash-proxy is a forward proxy written in Rust that sits between micro VM
guests and the outside world. It exists to make agent egress observable and
controllable.

## Use case

- Agents are run inside micro VMs.
- HTTP requests from the micro VM guests are forwarded to leash-proxy.
  - The traffic is controlled through [Cedar policies](/guide/policies).
- The host also runs nftables so only DNS and HTTP requests are passed from
  the micro VM guests.

## Design decisions

Architecture decisions are recorded as ADRs in
[`docs/adrs/`](https://github.com/akirakomamura/leash-proxy/tree/main/docs/adrs)
in the repository:

- [ADR 0001](https://github.com/akirakomamura/leash-proxy/blob/main/docs/adrs/0001-rust-mitm-proxy-with-cedar-policies.md)
  — Rust proxy with Cedar policies, fail-closed posture, structured audit
  logging, and credential hygiene.
- [ADR 0002](https://github.com/akirakomamura/leash-proxy/blob/main/docs/adrs/0002-event-storage-for-monitoring-ui.md)
  — JSONL logs are the source of truth; downstream indexing feeds the
  monitoring UI.
- [ADR 0003](https://github.com/akirakomamura/leash-proxy/blob/main/docs/adrs/0003-elixir-phoenix-downstream-collector-and-ui.md)
  — the Elixir/Phoenix collector and UI run as a separate downstream process.

## Next steps

- See [Architecture](/guide/architecture) for how the pieces fit together.
- Follow [Getting started](/guide/getting-started) to build and run the proxy.
- Learn how to write [Cedar policies](/guide/policies) for your agents.
