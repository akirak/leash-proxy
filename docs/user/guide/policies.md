# Cedar policies

Requests are authorized with [Cedar](https://www.cedarpolicy.com/). Policy
files are loaded and validated up front; any load or validation failure is
fail-closed and every request is denied.

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
