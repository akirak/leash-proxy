# leash-proxy

## Use case

- Agents are run inside micro VMs.
- HTTP requests from the micro VM guests are forwarded to leash-proxy.
  - The traffic is controlled through Cedar policies.
- The host also runs nftables so only DNS and HTTP requests are passed from the micro VM guests.
