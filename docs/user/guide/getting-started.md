# Getting started

## Nix packages

The flake provides two packages (without a default package target):

- `.#leash-proxy` — the Rust proxy binary (`$out/bin/leash-proxy`) and the
  default Cedar schema and policy files (`$out/share/leash-proxy/policies/`).
- `.#leash-collector` — the Phoenix LiveView audit console and Broadway log
  collector, built as a Mix release (`$out/bin/leash_collector`).

### Proxy (`leash-proxy`)

Build the proxy package with `nix build` or invoke it directly with `nix run`:

```console
$ nix build .#leash-proxy
$ ./result/bin/leash-proxy --help
```

The default policy paths (`policies/schema.cedar`, `policies/policies.cedar`)
are resolved relative to the process working directory. When running outside a
directory that contains `policies/`, pass `--schema` and `--policies` (or set
`policy.schema` and `policy.policies` in `--config`) pointing to your policy
files or the defaults bundled in the package:

```console
$ ./result/bin/leash-proxy \
    --listen 127.0.0.1:8080 \
    --schema ./result/share/leash-proxy/policies/schema.cedar \
    --policies ./result/share/leash-proxy/policies/policies.cedar \
    --log-file var/leash-proxy.jsonl
```

Or with `nix run`:

```console
$ nix run .#leash-proxy -- \
    --listen 127.0.0.1:8080 \
    --schema rust/policies/schema.cedar \
    --policies rust/policies/policies.cedar \
    --log-file var/leash-proxy.jsonl
```

### Collector and audit console (`leash-collector`)

Build the collector release:

```console
$ nix build .#leash-collector
```

The package exposes the Mix release entrypoint at `./result/bin/leash_collector`
(note the underscore in the executable name) and sets `mainProgram =
"leash_collector"` so `nix run .#leash-collector -- <command>` also works.

The release runs SQLite database migrations automatically on startup and is
configured via environment variables:

```console
$ mkdir -p var
$ RELEASE_COOKIE="$(openssl rand -base64 32)" \
  SECRET_KEY_BASE="$(openssl rand -base64 48)" \
  DATABASE_PATH=var/leash_collector.db \
  LEASH_AUDIT_LOG=var/leash-proxy.jsonl \
  PHX_SERVER=true \
  ./result/bin/leash_collector start
```

| Variable | Description |
| --- | --- |
| `PHX_SERVER` | Set to `true` to start the HTTP endpoint in a release. |
| `DATABASE_PATH` | Required. Path to the SQLite database file. |
| `SECRET_KEY_BASE` | Required. Secret key used to sign/encrypt cookies and LiveView state (e.g. `openssl rand -base64 48` or `mix phx.gen.secret`). |
| `RELEASE_COOKIE` | Required. Erlang distribution cookie (`mixRelease` omits a static `releases/COOKIE` file from `/nix/store`). |
| `LEASH_AUDIT_LOG` | Path to the proxy's append-only JSONL audit log (`tmp/audit.jsonl` by default). |
| `LEASH_AUDIT_CURSOR` | Optional checkpoint path (defaults to `.<log filename>.cursor` next to `LEASH_AUDIT_LOG`; its parent directory must be writable). |
| `PORT` | HTTP listener port (`4000` by default). |
| `PHX_HOST` | Public hostname (`example.com` by default). |
| `POOL_SIZE` | Ecto SQLite connection pool size (`5` by default). |
| `RELEASE_TMP` | Writable directory for runtime files when using `daemon` or `daemon_iex` (defaults to `$RELEASE_ROOT/tmp`, which is read-only in `/nix/store`). |

## Build and run from source

Cargo builds require OpenSSL development headers/libraries and `pkg-config`
(for example, `libssl-dev pkg-config` on Debian/Ubuntu). The Nix development
shell (`nix develop`) supplies the Rust, Elixir/Erlang, Lean 4, Node.js, and
`process-compose` toolchains:

```console
$ cargo run --manifest-path rust/Cargo.toml -- \
    --listen 127.0.0.1:8080 \
    --schema rust/policies/schema.cedar \
    --policies rust/policies/policies.cedar \
    --log-file var/leash-proxy.jsonl
```

To run both the proxy and the Phoenix audit console together in the
development shell:

```console
$ nix develop
$ (cd elixir && mix setup)
$ process-compose up
```

For transparent HTTP/HTTPS interception with your own CA, see
[Bring your own CA](/guide/configuration#bring-your-own-ca) and
[Transparent mode](/guide/configuration#transparent-mode). That guide includes
OpenSSL CA generation, guest trust setup, and Linux REDIRECT configuration.

See [Configuration](/guide/configuration) for all options.

## Tests

```console
$ cargo test --manifest-path rust/Cargo.toml
```

The test suite covers Cedar evaluation (allow, deny-by-default, explicit
forbid, fail-closed on bad schema/policies, unknown methods), redaction,
engine stacking, and end-to-end proxying (forwarding, CONNECT tunneling,
denies, malformed requests) over real TCP. Transparent interception tests
supply the recovered destination directly, without requiring root or changing
firewall rules; they cover CA trust, DNS/IP certificates, decrypted-request
policy enforcement, and upstream TLS verification. HTTP/2 coverage includes
ALPN negotiation, HTTP/1.1 interoperability on both hops, concurrent allowed
and denied streams, large bodies, trailers, h2c prior knowledge, and CONNECT.
