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

## Next steps

- See [Architecture](/guide/architecture) for how the pieces fit together.
- Follow [Getting started](/guide/getting-started) to build and run the proxy.
- Learn how to write [Cedar policies](/guide/policies) for your agents.
