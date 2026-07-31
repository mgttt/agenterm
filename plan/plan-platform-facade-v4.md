# Platform Facade revision-4 execution plan

状态：进行中。此计划收敛跨平台原生边界；它不授予或限制 Script Runtime
能力。调用方策略、预算和 typed failure 保持在上层产品合同。

## Outcome and dependency graph

```text
contract + selected adapters
├─ IPC / endpoint / stream                  [partial]
├─ system path conventions                  [adapter-owned]
├─ Script Runtime
│  ├─ process inventory + termination       [adapter-owned]
│  ├─ owned child tree                      [adapter-owned]
│  ├─ window interaction                    [adapter-owned]
│  ├─ clipboard / atomic files               [adapter-owned]
│  ├─ stream-handle probing                  [adapter-owned]
│  └─ worker supervision / audit             [adapter-owned]
├─ Control Center shell                       [partial]
├─ passive WebView runtime probe              [adapter-owned]
└─ frontend + PTY native lifecycle           [pending]
    └─ static source boundary gate           [depends on all above]
```

Shared prerequisites: typed `Unsupported` versus `Failed`, adapter-local native
handles, and contract tests that do not depend on a live GUI. Hot files
(`src/lib.rs`, `src/platform/mod.rs`, Cargo metadata and PRDs) are serialized;
each product module moves only after its facade service has an owned contract.

IPC implementation state: endpoint identity and selection are in
`contract::ipc`; transport failure codes and their endpoint-preserving error
carrier are now in `contract::ipc_transport`. `services::ipc` and every native
adapter consume that shared contract. The compatibility `ipc_transport` stream
still projects the legacy TCP framing and is the next deletion/migration leaf;
this state does not yet satisfy the static source-boundary gate.

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

## Shipped leaf: system path conventions

- User problem: product persistence and sidecar discovery must retain native
  path and executable-name conventions without embedding target selection in
  settings, workspace, client, or Control Center code.
- Invariant: `platform::paths` is compatibility-only; the selected adapter
  owns host font defaults, executable names, and workspace/settings/instance
  registry conventions. This is not caller authorization or a path allowlist.
- Delivery: `services::paths → selected → adapters/{windows,linux,macos}`;
  the root `platform::paths` module re-exports that service only.
- Evidence: focused path convention tests, settings and Control Center unit
  regressions, warnings-denied library Clippy, formatting, and a source scan
  showing no target selection or host environment convention in root/service
  path facades.
- Safe failure: existing deterministic fallback conventions remain unchanged;
  no new policy-based rejection is introduced.
- Public black-box owner: workspace persistence, Script worker discovery, and
  Control Center sidecar launch.
- Excluded scope: IPC transport mechanics, Control Center shell rendering, and
  terminal/frontend lifecycle.

## Remaining leaves and serial validation

1. Define typed Script-window, Script-clipboard, stream-probe, and atomic-file
   service contracts before moving each native implementation.
2. Move Control Center shell/focus/capture and WebView host internals behind
   their services, retaining bounded deadlines and typed failures. The shell
   split keeps registry, IPC projection, and receipts in `control_center`; a
   narrow projection-host contract supplies title, lines, polling, close,
   native-window publication, focus requests, and typed capture requests to
   the selected adapter driver. It must not duplicate snapshot or registry
   identity logic in an OS adapter.
3. Split PTY/frontends into adapter-owned event-loop and native-terminal
   lifecycle implementations; product state stays platform-neutral. The PTY
   contract exposes terminal size, spawn specification, typed exit/failure,
   and independent session/reader/wait operations without native handles.
   POSIX `openpty`/fork/session/exec/poll and Windows ConPTY/job mechanics stay
   below the selected adapter, preserving the existing reader/wait concurrency
   and terminate-to-EOF ordering.
   POSIX mechanics are now physically adapter-owned; Windows wrapper type
   conversion remains the blocking leaf before `src/pty` can lose its final
   compatibility projection.
   The first frontend leaf is complete: runtime-primary shell descriptors now
   select in adapters, so the Unix new-terminal dialog contains no macOS/Linux
   conditional or shell-path constant. Unix frontend clipboard selection also
   now consumes a typed facade service, as does XRGB screenshot encoding;
   font candidate selection is likewise adapter-owned; renderer/input/event-
   loop migration is still pending.
4. Remove compatibility-only legacy native paths after each owning public
   smoke has passed.
5. Add the static production source boundary test only after no product native
   escape hatches remain. It rejects OS cfg/native API imports outside the
   approved platform adapters, required bin entry points, and tests.
6. Run serial integrated `fmt`, Clippy, unit tests, owning public CLI smoke,
   boundary scan, then the applicable Windows qualification lane. No Candidate,
   tag, or public release is implied by this plan.
