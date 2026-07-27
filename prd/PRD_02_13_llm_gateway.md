# Local LLM gateway (`agenterm-llm-gateway.exe`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- Dependency and isolation
  - [ ] implementation begins only after Observable Fleet, Rhai capability
    policy, MCP typed tools, credential isolation, and audit contracts pass
    their gates
  - [ ] run as an optional loopback-authenticated sidecar, separate from the
    lightweight specialized-model worker and from GUI startup
  - [ ] keep provider credentials in an OS credential store, outside
    workspaces, tab environments, scripts, and child-process inheritance
  - [ ] prompt and response bodies are not logged by default; PTY content
    requires explicit scoped authorization, redaction, and bounded lifetime
- Governed forwarding
  - [ ] support provider and local endpoints through destination allowlists,
    policy routing, per-workspace/tab/agent quotas, token and monetary
    budgets, deadlines, retry/idempotency, circuit breaking, health checks,
    streaming cancellation, and policy-controlled fallback
  - [ ] audit provider/model route, latency, actual or estimated token use,
    versioned price basis, cost, policy decision, and denial reason without
    recording credentials or content secrets
  - [ ] prefer provider-reported usage to estimates and reconcile retries so
    cancellation or duplicate attempts cannot silently hide cost
  - [ ] LLM text is never the sole proof of a successful fleet operation;
    MCP tools verify typed post-state through the AgenTerm control plane
