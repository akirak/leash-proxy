---
layout: home

hero:
  name: leash-proxy
  tagline: A Rust forward proxy for micro VM agents, controlled by Cedar policies with structured audit logging.
  actions:
    - theme: brand
      text: Get started
      link: /guide/getting-started
    - theme: alt
      text: Why leash-proxy?
      link: /guide/introduction
    - theme: alt
      text: Cedar policies
      link: /guide/policies

features:
  - title: Policy-controlled egress
    details: HTTP requests from micro VM guests are forwarded through the proxy and evaluated against Cedar policies with a fail-closed posture.
  - title: Agent identification
    details: Guest source IPs are resolved to agent ids, which become Cedar principals for per-agent authorization.
  - title: Structured audit logging
    details: Every request produces one JSONL record with the decision, determining policies, bytes transferred, and latency.
---
