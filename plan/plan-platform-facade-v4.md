# Platform Facade revision-4 execution plan

状态：完成（2026-08-01）。此计划已收敛跨平台原生边界；它不授予或限制
Script Runtime 能力。调用方策略、预算和 typed failure 保持在上层产品合同。

## Outcome and dependency graph

```text
contract + selected adapters
├─ IPC / endpoint / stream                  [adapter-owned]
├─ system path conventions                  [adapter-owned]
├─ Script Runtime
│  ├─ process inventory + termination       [adapter-owned]
│  ├─ owned child tree                      [adapter-owned]
│  ├─ window interaction                    [adapter-owned]
│  ├─ clipboard / atomic files               [adapter-owned]
│  ├─ stream-handle probing                  [adapter-owned]
│  └─ worker supervision / audit             [adapter-owned]
├─ Control Center shell                       [adapter-owned]
├─ passive WebView runtime probe              [adapter-owned]
└─ frontend + PTY native lifecycle           [adapter-owned]
    └─ static source boundary gate           [passing]
```

Shared prerequisites: typed `Unsupported` versus `Failed`, adapter-local native
handles, and contract tests that do not depend on a live GUI. Hot files
(`src/lib.rs`, `src/platform/mod.rs`, Cargo metadata and PRDs) are serialized;
each product module moves only after its facade service has an owned contract.

The production module graph now has one assembly point: `selected.rs` chooses
the target and mounts both service adapters and their private `native/`
mechanisms. The former top-level `platform/{windows,linux,macos}` trees are
physically folded into `adapters/{windows,linux,macos}/native`; shared
`platform/mod.rs` performs no production OS selection. A second static gate
enforces that platform-internal OS cfg and native API markers remain in
`selected.rs` or adapters, keeping contracts and services platform-neutral.

IPC implementation state: endpoint identity and selection are in
`contract::ipc`; transport failure codes and their endpoint-preserving error
carrier are in `contract::ipc_transport`. `services::ipc` and every native
adapter consume that shared contract. The compatibility `ipc_transport` stream
now contains only platform-neutral TCP/framing/server behavior; its duplicate
Unix-socket and Windows named-pipe implementations were removed after the
selected-adapter round-trip tests passed. The static source-boundary gate also
passes this state.

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

## Closure leaves and serial validation

1. Typed Script-window, Script-clipboard, stream-probe, and atomic-file
   service contracts are complete and their native implementations are
   adapter-owned.
2. Control Center shell/focus/capture and WebView host internals now sit behind
   their services with bounded deadlines and typed failures. The shell split
   keeps registry, IPC projection, screenshot receipts, and snapshot identity
   in `control_center`; a narrow projection-host contract supplies title,
   lines, polling, close, native-window publication, focus requests, and typed
   capture requests to selected adapter drivers. Windows no-activate open,
   760×480 native capture, public close, and orphan-free status pass locally.
3. Split PTY/frontends into adapter-owned event-loop and native-terminal
   lifecycle implementations; product state stays platform-neutral. The PTY
   contract exposes terminal size, spawn specification, typed exit/failure,
   and independent session/reader/wait operations without native handles.
   POSIX `openpty`/fork/session/exec/poll and Windows ConPTY/job mechanics stay
   below the selected adapter, preserving the existing reader/wait concurrency
   and terminate-to-EOF ordering.
   POSIX mechanics are now physically adapter-owned; the Windows adapter owns
   `rmux-pty` and converts neutral size/process identities. `src/pty` contains
   no target selection and projects only `services::pty`; lifecycle operations
   now distinguish typed Unsupported from stable-code Failed results while
   byte-stream I/O remains standard I/O.
   The first frontend leaf is complete: runtime-primary shell descriptors now
   select in adapters, so the Unix new-terminal dialog contains no macOS/Linux
   conditional or shell-path constant. Unix frontend clipboard selection also
   now consumes a typed facade service, as does XRGB screenshot encoding;
   font candidate selection is likewise adapter-owned. The Windows launcher
   and replaceable native GUI projection are physically adapter-owned and
   selected through the frontend service. The complete winit/softbuffer event
   loop, input, renderer, font cache, dialogs, screenshot bridge, and wake proxy
   now reside in the private Unix adapter mechanism selected by explicit Linux
   and macOS entries. Shared UI-state normalization remains open.
   The final dead `src/pty/windows.rs` third-party type re-export is deleted;
   the source gate now rejects `rmux-pty`, winit, softbuffer, and raw-window-
   handle types outside adapters in addition to direct native APIs.
4. Remove compatibility-only legacy native paths after each owning public
   smoke has passed. The legacy native IPC transport copies are removed; the
   remaining compatibility module is platform-neutral protocol/server code.
5. The static production source boundary test now rejects OS cfg/native API
   imports outside the approved platform tree. It scans every Rust source,
   structurally masks test items/comments/strings, and allows only the three
   exact Windows-subsystem entry attributes; its rejection fixture and clean
   repository scan pass locally. The same test binary loads OS-neutral
   declarations from all three adapter trees and verifies the common revision,
   complete capability surface, and non-empty typed Unsupported/Failed probes
   on every host.
6. Serial integrated validation passes on the final implementation tree:
   repository lint, `fmt`, warnings-denied all-target Clippy, 389 library tests,
   both boundary scans, and the three-adapter same-host contract test; the dev
   build stages all seven Windows binaries. Public CLI `--help`/`protocol-info`,
   native IPC smoke, and the complete Control Center smoke pass, including a
   760×480 native capture, no-activate reuse, recovery, exact typed close, and
   orphan-free cleanup. A Windows-host Linux-target probe stops in dependency
   build setup because `x86_64-linux-gnu-gcc` is not installed; it supplies no
   contrary source result and native/cross target coverage remains owned by CI.
   No Candidate, tag, or public release was run or implied by this plan.
