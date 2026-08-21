# tinyvm native completion channel

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Status: host-neutral core, reusable guest protocol and iOS C/Swift owner implemented

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

A versioned native module defines its module-specific start imports and may
register the common sibling protocol atomically through
`NativeModuleRegistry::register_completion_imports`:

```text
completion_poll(ticket, status_ptr, length_ptr) -> code
completion_take(ticket, destination_ptr, capacity) -> code
completion_cancel(ticket) -> code

code 0 = pending
code 1 = ready / consumed
code 2 = stale ticket
code 3 = destination too small
```

Poll preflights two non-overlapping four-byte outputs and writes the native
status plus payload length only when ready. Take preserves a pending result or
a result that does not fit; it validates the complete guest range before
removing the owned payload. Cancel invalidates pending or ready work. Invalid
guest memory remains a VM trap because it is malformed Wasm host-call state,
while ordinary queue states remain stable result codes. Registration requires
the queue's assigned domain to belong to the same native module, reserves all
three function slots, and rejects collisions before publishing any of them.

The native module still specifies request arguments, scheduling, native status
values and replay behavior. The common protocol does not invent engine-private
imports and does not make asynchronous external results deterministic. If
replay requires them, the embedding records the normalized completion as host
input at a lifecycle boundary.

This adopts QJWasm's useful separation between call, callback and completion
channels without adopting QuickJS, a two-runtime ownership graph, or its thread
protocol as an iOS dependency.

## iOS ownership boundary

C ABI v1.10 exposes an opaque `tinyarcade_completion_v1` independently of the
runtime handle. The app creates it first, captures it in the module-specific
native `start` callback, calls `completion_begin`, and returns that ticket to
the guest. Platform work remains app-owned. The app copies its final payload
through `completion_complete` only after marshalling back to the runtime owner
thread. Swift exposes the same model as the `@MainActor`
`TinyArcadeCompletionV1` owner.

The channel binds to at most one live runtime. Closing it while bound fails;
closing the runtime clears every pending/ready request and unbinds it, so a
late platform result receives an explicit error without dereferencing a dead
runtime. The runtime borrows the channel handle until close, while Swift retains
the channel for that exact interval. `copy_host_profile_with_completions`
publishes the ordinary start function and all three common imports, keeping
converter preflight identical to runtime binding.
