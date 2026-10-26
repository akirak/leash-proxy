# Configuration

CLI flags override a TOML file, which overrides defaults.

```toml
[server]
listen = "0.0.0.0:8080"
mode = "forward"                  # or "transparent" (Linux REDIRECT)
# ca_dir = "/etc/leash-proxy/ca"   # required for transparent mode

[agents]
# static source IP -> agent id mapping used for Cedar principals
"10.0.0.5" = "vm-alpha"

[policy]
schema = "policies/schema.cedar"
policies = ["policies/policies.cedar"]

[logging]
log_file = "var/leash-proxy.jsonl"   # stdout when absent
header_allowlist = ["host", "user-agent", "accept", "content-type", "content-length"]
```

## Keys

| Key | Description |
| --- | ----------- |
| `server.listen` | Address and port the proxy listens on (`--listen`). |
| `server.mode` | `forward` (default) or Linux `transparent` interception (`--mode`). |
| `server.ca_dir` | Existing CA directory (`--ca-dir`), required in transparent mode. |
| `agents` | Static source IP → agent id mapping used for Cedar principals. |
| `policy.schema` | Path to the Cedar schema file. |
| `policy.policies` | Paths to the Cedar policy set files (Cedar text or Cedar JSON). |
| `logging.log_file` | JSONL log destination; stdout when absent. |
| `logging.header_allowlist` | Headers that may be recorded in audit logs. |

## Bring your own CA

Use `--ca-dir /etc/leash-proxy/ca` or `server.ca_dir` to supply an existing
certificate authority. Relative paths are resolved from the process working
directory, just like policy paths. The directory must contain:

- `ca-cert.pem`: PEM CA certificate, with the signing certificate first and
  any intermediate/root certificates appended after it.
- `ca-key.pem`: matching **unencrypted** PEM RSA or EC private key. PKCS#1,
  PKCS#8, and SEC1 keys are supported. Password-protected keys are rejected
  without prompting.

The certificate must be currently valid, have `basicConstraints = CA:TRUE`,
and permit certificate signing if it has a `keyUsage` extension. Missing,
malformed, expired, non-CA, or mismatched material fails startup before the
listener is opened, even if `--ca-dir` is supplied in forward mode. There is
no fallback CA generation, and no files in this directory are modified or
created. Keep the directory private (0700) and the key readable only by the
proxy's service account (0600); a read-only secret mount is supported.
Restart the proxy after replacing CA files.

### Generate a CA with OpenSSL

Create a new directory, then generate a self-signed CA (OpenSSL 1.1.1+):

```sh
mkdir -m 700 leash-ca
(
  umask 077
  openssl req -x509 -newkey rsa:3072 -sha256 -nodes -days 3650 \
    -subj '/CN=Leash Proxy CA' \
    -addext 'basicConstraints=critical,CA:TRUE' \
    -addext 'keyUsage=critical,keyCertSign,cRLSign' \
    -keyout leash-ca/ca-key.pem \
    -out leash-ca/ca-cert.pem
)
```

Distribute **only the public CA certificate** to the micro VM guests and add
it to their OS/application trust stores. Never copy the private key into a
guest. For a one-off curl test, `--cacert /path/to/ca-cert.pem` supplies the
trust anchor; do not use `--insecure`.

To reuse a mitmproxy CA, place its `mitmproxy-ca-cert.pem` at `ca-cert.pem`
and its private-key-containing `mitmproxy-ca.pem` at `ca-key.pem` in a
private directory. The extra certificate in mitmproxy's combined PEM file
is ignored when reading the key.

## Transparent mode

```sh
leash-proxy --mode transparent --listen 0.0.0.0:8080 \
  --ca-dir /etc/leash-proxy/ca \
  --schema /path/to/schema.cedar --policies /path/to/policies.cedar
```

Transparent mode accepts origin-form HTTP and terminates HTTPS with
short-lived, per-connection DNS/IP SAN certificates signed by your CA.
Certificates and leaf keys remain in memory. Every decrypted HTTP request
is evaluated by Cedar before connecting upstream; audit records use scheme
`https` and TLS metadata `terminated`. Policies must permit the actual HTTP
method, host, port, and path, not just `http_connect`.

On a Linux gateway, redirect the guests' TCP HTTP/HTTPS traffic to the proxy
using netfilter REDIRECT, as for mitmproxy's Linux transparent mode. For
example, replacing `vmbr0` with the guest-facing interface:

```sh
sudo iptables -t nat -A PREROUTING -i vmbr0 -p tcp \
  -m multiport --dports 80,443 -j REDIRECT --to-ports 8080
```

The proxy must run on the gateway performing REDIRECT, and the guests'
routes must send their traffic through that gateway. Configure firewall
input/forwarding rules separately; leash-proxy does not change routes,
firewall rules, or IP forwarding settings. For IPv6 use equivalent
`ip6tables` REDIRECT rules and an IPv6 listener. Avoid redirecting the proxy's
own upstream traffic back into itself.

The original destination IP and port are recovered from the kernel using
`SO_ORIGINAL_DST` (or its IPv6 equivalent), and upstream connections use that
destination rather than re-resolving Host/SNI. HTTPS uses SNI, or the original
destination IP when SNI is absent, for certificate issuance and upstream
identity verification. Host/SNI disagreement and explicit Host ports that
differ from the intercepted destination are rejected. Upstream certificates
are verified against the **OS trust store**, not automatically trusted because
the interception CA signed them. Failure to recover the destination fails
closed; there is no opaque TCP fallback.

### HTTP/1.1 and HTTP/2

HTTPS interception advertises `h2` and `http/1.1` through TLS ALPN, preferring
HTTP/2. The upstream TLS connection negotiates independently: HTTP/2 clients
can reach HTTP/1.1 origins and HTTP/1.1 clients can reach HTTP/2 origins.
Origins without ALPN fall back to HTTP/1.1. No additional flags are needed.

Multiplexed HTTP/2 requests are checked and audited separately, including
denied streams on otherwise allowed connections. The `:authority` pseudo-header
is used for destination policy checks and must agree with SNI and any supplied
Host header. The `:scheme` must match the intercepted transport. Streaming
bodies and HTTP/2 trailers are forwarded without buffering; `TE: trailers`
is retained when sending HTTP/2 requests.

The forward and transparent listeners also accept cleartext HTTP/2
prior-knowledge connections (h2c). Cleartext origin connections use HTTP/1.1;
HTTP/1.1 `Upgrade: h2c` negotiation is not supported. Forward-mode CONNECT
works over either HTTP/1.1 or HTTP/2 and remains an opaque tunnel.

QUIC/HTTP/3, non-HTTP TLS, client-certificate authentication, and
certificate-pinned clients are not supported. Transparent mode currently
supports Linux REDIRECT, not TPROXY or other operating systems' transparent
mechanisms. Configure guests/firewalls to prevent bypass over
unintercepted protocols (for example UDP/443) if enforcing all egress.
Forward mode remains the default and continues to tunnel CONNECT without
TLS interception, even when `--ca-dir` is supplied.

See [Cedar policies](/guide/policies) for how agents and requests map to
Cedar entities, and [Audit log](/guide/audit-log) for what gets logged.
