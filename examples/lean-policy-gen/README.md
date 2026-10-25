# lean-policy-gen

Example that generates a leash-proxy Cedar policy set in the [Cedar JSON policy
format (the EST)](https://docs.cedarpolicy.com/policies/json-format.html) from
Lean 4.

leash-proxy accepts policy files in either Cedar format: a file whose content
starts with `{` is loaded as JSON. This example shows the Lean side of that
pipeline — Lean values → `Lean.Json` → policy file → leash-proxy — using a
small, EST-shaped policy AST defined in `Policies.lean`:

- `Expr` — Cedar expressions (`Var`, `Value`, `==`, `&&`, `in`, `.`, `has`,
  `like`, ...), each serializing to its EST JSON encoding
- `Scope` — principal/action/resource scope constraints (`All`, `==`, `in`)
- `Policy` / `policySet` — a policy (effect, scopes, `when`/`unless`
  conditions) and a whole static policy set

## Build and run

```console
$ nix develop            # provides lean4 and the Rust toolchain
$ lake build
$ lake exe gen           # writes policies.json (3 policies)
```

`policies.json` is the checked-in output of `lake exe gen`; it is equivalent
to the default Cedar text policy set at `rust/policies/policies.cedar`, and the
Rust test suite (`rust/tests/cedar_json_policies.rs`) loads it and asserts
behavior parity with the text version.

## Use it with leash-proxy

```console
$ lake exe gen /tmp/policies.json
$ cargo run --manifest-path ../../rust/Cargo.toml -- \
    --schema ../../rust/policies/schema.cedar \
    --policies /tmp/policies.json
```

## Notes

- The Lean types guarantee the output is *well-formed JSON in the EST shape*,
  not that it matches the Cedar schema. The proxy loads and validates policy
  sets fail-closed, and `cedar validate` can be used in CI for the same
  check.
- Cedar's JSON policy set format requires the `templates` and `templateLinks`
  keys to be present even when empty; `policySet` emits them.
- This is the "decoupled" approach: the AST is defined here rather than
  reused from the Cedar formal specification. If you later want to prove
  properties about the policies you emit, the types would move toward
  `cedar-spec`'s model instead.
- Templates (`?principal` slots) and template links are not covered by this
  example's Lean types, though the proxy does load JSON policy sets that
  contain them.
