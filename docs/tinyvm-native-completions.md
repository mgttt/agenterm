# tinyvm native completion channel

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Status: host-neutral core implemented; native-module import protocols remain module-specific

`HostCompletionQueue` is the common `no_std + alloc` state machine for native
work that cannot finish inside a synchronous Wasm import. It deliberately owns
no thread, executor, Promise, wake primitive or platform API. iOS, Windows,
Unix, a JavaScript development oracle, or another host may schedule work by its
normal mechanism and marshal the result back to the runtime owner before
completing the request.

```text
owner/runtime thread
├── begin(max_payload_bytes)
│   ├── reserve one bounded table slot
│   ├── reserve response bytes before external work
│   └── return domain + generation checked i32 ticket
├── platform work outside tinyvm
│   └── completion is marshalled back to this owner
├── try_complete(ticket, status, Vec<u8>)
│   ├── zero-copy payload ownership transfer on success
│   └── rejected payload ownership returned to host
└── poll(ticket) → Pending | Ready(&[u8])
    └── take/cancel invalidates ticket and releases reservation
```

The queue is created through `NativeModuleRegistry::completion_queue`, so it
inherits `HostResourceTable` identity and lifecycle rules. Tickets from sibling
or replacement runtimes never alias. Pending work and completed-but-unclaimed
results are live native resources; portable suspend fails until the guest/host
protocol takes or cancels them.

Both dimensions are bounded before work begins:

- `max_pending` limits outstanding item count;
- `max_reserved_bytes` limits aggregate response ownership;
- each `begin` fixes that request's maximum payload;
- saturation, stale tickets, duplicate completion, premature take and oversized
  results return distinct typed errors;
- arithmetic overflow is the same explicit byte-budget failure;
- `clear`/drop releases payload ownership and all reservations.

A versioned native module still defines its own ordinary Wasm imports, for
example `start(...) -> i32`, `poll(ticket, ...) -> i32` and `close(ticket) ->
i32`. That contract must specify status values, guest-memory copy bounds,
cancellation and replay behavior. The common queue does not invent engine-
private imports and does not make asynchronous external results deterministic.
If replay requires them, the embedding must record the normalized completion as
host input at a lifecycle boundary.

This adopts QJWasm's useful separation between call, callback and completion
channels without adopting QuickJS, a two-runtime ownership graph, or its thread
protocol as an iOS dependency.
