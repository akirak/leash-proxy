# Getting started

## Build and run

```console
$ nix build              # or: cargo build --manifest-path rust/Cargo.toml
$ ./result/bin/leash-proxy --help
```

Cargo builds require OpenSSL development headers/libraries and `pkg-config`
(for example, `libssl-dev pkg-config` on Debian/Ubuntu). The Nix development
shell supplies these dependencies.

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
