# leash-proxy

leash-proxy is an egress proxy written in Rust that sits between micro VM
guests and the outside world. It supports HTTP forwarding, CONNECT tunnels,
and Linux transparent HTTP/HTTPS interception (HTTP/1.1 and HTTP/2) with a
[bring-your-own CA](docs/user/guide/configuration.md#bring-your-own-ca).
Requests are evaluated against [Cedar](https://www.cedarpolicy.com/) policies with a fail-closed posture, and every request is recorded as a
structured JSONL audit record.

## Documentation

The user documentation is a [VitePress](https://vitepress.dev) site in
[`docs/user/`](docs/user/):

- [Introduction](docs/user/guide/introduction.md) — use case and design decisions
- [Architecture](docs/user/guide/architecture.md)
- [Getting started](docs/user/guide/getting-started.md) — build, run, and test
- [Configuration](docs/user/guide/configuration.md)
- [Cedar policies](docs/user/guide/policies.md)
- [Audit log](docs/user/guide/audit-log.md)
- [Roadmap](docs/user/roadmap.md)

To work on the documentation locally:

```console
$ npm install --prefix docs/user
$ npm run dev --prefix docs/user       # dev server
$ npm run build --prefix docs/user     # static build into docs/user/.vitepress/dist
```

## Local development

Enter the Nix development shell, set up the console once (or after changing
its dependencies), then start both servers with
[process-compose](https://github.com/F1bonacc1/process-compose):

```console
$ nix develop
$ (cd elixir && mix setup)
$ process-compose up
```

The proxy listens on `127.0.0.1:8081` and appends audit events to
`var/leash-proxy.jsonl`. The Phoenix audit console tails that file and serves
the UI at <http://localhost:4000>. process-compose's own TUI/API uses port
`8080`. Stop everything with `Ctrl-C` (or `process-compose down` from another
terminal).

## Repository layout

- [`rust/`](rust/) — the proxy library (`leash_proxy`) and binary (`leash-proxy`)
- [`elixir/`](elixir/) — Phoenix LiveView audit console and Broadway collector
  with batched SQLite storage; see its [setup guide](elixir/README.md)
- [`examples/lean-policy-gen/`](examples/lean-policy-gen/) — Lean 4 example
  that generates a Cedar policy set in the JSON (EST) format
- [`docs/adrs/`](docs/adrs/) — architecture decision records
- [`docs/user/`](docs/user/) — user documentation site (VitePress)

## Inspirations

- [mitmproxy](https://github.com/mitmproxy)
- [strongdm/leash](https://github.com/strongdm/leash)
