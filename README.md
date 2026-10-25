# leash-proxy

leash-proxy is a forward proxy written in Rust that sits between micro VM
guests and the outside world. HTTP requests from the guests are forwarded
through the proxy and evaluated against [Cedar](https://www.cedarpolicy.com/)
policies with a fail-closed posture, and every request is recorded as a
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

## Repository layout

- [`rust/`](rust/) — the proxy library (`leash_proxy`) and binary (`leash-proxy`)
- [`elixir/`](elixir/) — Phoenix LiveView audit console and Broadway collector
  with batched SQLite storage; see its [setup guide](elixir/README.md)
- [`examples/lean-policy-gen/`](examples/lean-policy-gen/) — Lean 4 example
  that generates a Cedar policy set in the JSON (EST) format
- [`docs/adrs/`](docs/adrs/) — architecture decision records
- [`docs/user/`](docs/user/) — user documentation site (VitePress)
