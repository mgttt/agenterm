# Platform Facade revision-4 execution plan

状态：进行中。此计划收敛跨平台原生边界；它不授予或限制 Script Runtime
能力。调用方策略、预算和 typed failure 保持在上层产品合同。

## Outcome and dependency graph

```text
contract + selected adapters
├─ IPC / endpoint / stream                  [partial]
├─ Script Runtime
│  ├─ process inventory + termination       [complete slice]
│  ├─ owned child tree                      [complete slice]
│  ├─ window interaction                    [pending]
│  ├─ clipboard / stream / atomic files     [pending]
│  └─ worker supervision / audit            [pending]
├─ Control Center / WebView shell            [partial]
└─ frontend + PTY native lifecycle           [pending]
    └─ static source boundary gate           [depends on all above]
```

Shared prerequisites: typed `Unsupported` versus `Failed`, adapter-local native
handles, and contract tests that do not depend on a live GUI. Hot files
(`src/lib.rs`, `src/platform/mod.rs`, Cargo metadata and PRDs) are serialized;
each product module moves only after its facade service has an owned contract.

## Shipped leaf: Script Runtime process inventory and termination

- User problem: scripts need to list and terminate operating-system processes
  without encoding Win32, `/proc`, or macOS C APIs in the script product layer.
- Invariant: `std.process.list` / `std.process.kill` preserve their typed Rhai
  receipt categories and do not implement caller permission policy.
- Delivery: `script_process.rs` maps `platform::process::{list,kill}` typed
  results into existing public error codes; adapter-native inventory and kill
  mechanics have one owner beneath `platform`.
- Evidence: focused `script_process::tests`, warnings-denied library Clippy,
  formatting, and source scan showing this slice has no process-inventory or
  process-termination native calls.
- Safe failure: typed `process_list_*` / `process_kill_*` error, including
  explicit Unsupported where an adapter cannot provide the operation.
- Public black-box owner: `agenterm-script` `std.process` API.
- Excluded scope: top-level window inspection/control, clipboard, stream-handle
  probing, filesystem replacement, and any authorization policy.

## Remaining leaves and serial validation

1. Define typed Script-window, Script-clipboard, stream-probe, and atomic-file
   service contracts before moving each native implementation.
2. Move Control Center shell/focus/capture and WebView host internals behind
   their services, retaining bounded deadlines and typed failures.
3. Split PTY/frontends into adapter-owned event-loop and native-terminal
   lifecycle implementations; product state stays platform-neutral.
4. Remove compatibility-only legacy native paths after each owning public
   smoke has passed.
5. Add the static production source boundary test only after no product native
   escape hatches remain. It rejects OS cfg/native API imports outside the
   approved platform adapters, required bin entry points, and tests.
6. Run serial integrated `fmt`, Clippy, unit tests, owning public CLI smoke,
   boundary scan, then the applicable Windows qualification lane. No Candidate,
   tag, or public release is implied by this plan.
