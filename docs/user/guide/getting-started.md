# Getting started

## Build and run

```console
$ nix build              # or: cargo build --manifest-path rust/Cargo.toml
$ ./result/bin/leash-proxy --help
```

The default policy files are resolved relative to the working directory
(`policies/schema.cedar`, `policies/policies.cedar`); the nix package ships
them under `$out/share/leash-proxy/policies`.

```console
$ cargo run --manifest-path rust/Cargo.toml -- \
    --listen 0.0.0.0:8080 \
    --schema policies/schema.cedar \
    --policies policies/policies.cedar \
    --log-file var/leash-proxy.jsonl
```

See [Configuration](/guide/configuration) for all options.

## Tests

```console
$ cargo test --manifest-path rust/Cargo.toml
```

The test suite covers Cedar evaluation (allow, deny-by-default, explicit
forbid, fail-closed on bad schema/policies, unknown methods), redaction,
engine stacking, and end-to-end proxying (forwarding, CONNECT tunneling,
denies, malformed requests) over real TCP.
