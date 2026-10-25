# Cedar policies

Requests are authorized with [Cedar](https://www.cedarpolicy.com/). Policy
files are loaded and validated up front; any load or validation failure is
fail-closed and every request is denied.

Policy files may be written in either of Cedar's two formats:

- the **Cedar policy language** ("Cedar text"), e.g. `policies.cedar`;
- the **Cedar JSON policy format** (the EST), e.g. `policies.json`.

A policy file whose content starts with `{` is loaded as JSON, so the two
formats can be mixed freely within one deployment; every configured file is
merged into a single policy set before validation. The JSON format is what
external tools emit, which makes it a natural target for generating policies
programmatically (see [Generating policies from Lean 4](#generating-policies-from-lean-4)).

## Policy model

The default schema maps a proxied request onto Cedar entities as follows (see
`rust/policies/` in the repository):

| Cedar slot  | Value                                |
| ----------- | ------------------------------------ |
| principal   | `Agent::<agent id>`                  |
| action      | `Action::"http_<method>"`            |
| resource    | `Host::<destination>` (port, scheme) |
| context     | `{ path }`                           |

The agent id comes from the source IP of the micro VM guest, resolved via the
static mapping in the [configuration](/guide/configuration#keys).

## Example policy

```cedar
permit(principal == Agent::"vm-build", action == Action::"http_get", resource == Host::"pypi.org")
when { context.path like "/simple/*" };
```

This permits the agent `vm-build` to issue `GET` requests to `pypi.org` for
paths starting with `/simple/`. Everything not explicitly permitted is denied.

## JSON policy format

The same policy in the Cedar JSON policy format (a static policy set with
mandatory empty `templates` and `templateLinks` sections):

```json
{
  "staticPolicies": {
    "pypi-simple": {
      "effect": "permit",
      "principal": {"op": "==", "entity": {"type": "Agent", "id": "vm-build"}},
      "action": {"op": "==", "entity": {"type": "Action", "id": "http_get"}},
      "resource": {"op": "==", "entity": {"type": "Host", "id": "pypi.org"}},
      "conditions": [
        {
          "kind": "when",
          "body": {
            "like": {
              "left": {".": {"left": {"Var": "context"}, "attr": "path"}},
              "pattern": [{"Literal": "/"}, "Wildcard"]
            }
          }
        }
      ]
    }
  },
  "templates": {},
  "templateLinks": []
}
```

Point `--policies` (or `[policy] policies` in the config file) at the JSON
file and it is loaded like the text equivalent. The official Cedar CLI can
convert between the formats (`cedar translate-policy --direction
json-to-cedar`), which is handy for eyeballing generated policies.

## Generating policies from Lean 4

The repository contains a self-contained example that generates a Cedar JSON
policy set from [Lean 4](https://lean-lang.org/) (see
`examples/lean-policy-gen/`):

```console
$ cd examples/lean-policy-gen
$ lake exe gen            # writes policies.json
$ cargo run --manifest-path ../../rust/Cargo.toml -- \
    --schema ../../rust/policies/schema.cedar \
    --policies policies.json
```

Policies are authored against a small typed AST (`Expr`, `Scope`, `Policy`)
that mirrors the EST encoding, with a serializer into `Lean.Json`. This gives
Lean's type checking and editor support while authoring policies, and lets
you factor out helpers (`httpGet`, `agent "vm-build"`, ...) into a reusable
policy vocabulary. The example generates a JSON policy set equivalent to the
default Cedar text policy set in `rust/policies/policies.cedar`, and the Rust
test suite loads it to keep the two in sync.

The Lean types only guarantee that the output is well-formed JSON in the EST
shape, not that it matches your entity types and attributes — the proxy's
fail-closed load and `cedar validate` remain the authority on schema
conformance.
