# Precision audit — narrow, high-confidence findings

Tracks a targeted second-pass review running alongside the ongoing
frontend-parity refactor. Scope is deliberately narrow: real correctness/
safety bugs and untested trust boundaries with limited, well-understood
blast radius — not architecture or style cleanup (that's covered by the
existing `plan-v0.1.13.md`/`ARCHITECTURE.md` tracks).

Process: investigate one item at a time, avoid files another agent has open
(check `git status` first), verify with `cargo check`/`clippy -D warnings`
(and tests where the host target allows), commit each landed item
separately, then update this table.

## Findings

| # | Area | File(s) | Status | Notes |
|---|------|---------|--------|-------|
| 1 | fork()/execve() async-signal-safety | `crates/agenterm-platform/src/adapters/linux/pty.rs` (shared by `macos/pty.rs` via `#[path]`) | **Fixed** (`8e1e413`) | Heap allocation (CString/Vec/setenv) after `fork()`, before `execve()`, in a possibly-multithreaded process. Moved all allocation before `fork()`; child branch is now raw libc calls only. Also removed a dead post-fork `setenv` loop that never affected the `envp` already passed to `execve`. |
| 2 | Named-pipe overlapped I/O cancellation | `crates/agenterm-platform/src/adapters/windows/ipc.rs` | **Audited, no bug** | Checked for the classic stack-`OVERLAPPED` use-after-free on timeout (kernel completes cancelled I/O after the stack frame is gone). `wait_overlapped` calls `CancelIoEx` then a *blocking* `GetOverlappedResult` to synchronize before returning — correct. |
| 3 | Cross-process HANDLE duplication | `crates/agenterm-platform/src/adapters/windows/process_reference.rs` | **Audited, no bug** | `RemoteHandleTransfer` uses a rollback-on-drop pattern (duplicates the handle back and closes it in the remote process unless explicitly committed), with a dedicated test (`remote_handle_transfer_rolls_back_until_committed`). Solid. |
| 4 | IPC trust-boundary logic has zero unit tests | `src/control_authority.rs` (313 lines: admit/complete/finish_submission, replay/idempotency) | Investigating | Read once; no bug found yet by inspection, but it's the one module sitting directly on the control-protocol replay/dedupe boundary with no direct test coverage (only exercised indirectly through end-to-end IPC paths). Next: read `ReplayWindow` in `control_contract.rs` test module for existing coverage shape, then decide between (a) adding targeted regression tests for edge cases (capacity eviction, mismatched dedupe key, `Accepted`→never-finalized), or (b) closing this out if coverage turns out adequate. |
| 5 | Highest unsafe density, zero `SAFETY` comments | `crates/agenterm-platform/src/adapters/windows/control_window.rs` (93 `unsafe` blocks) | Not started | Largest unsafe surface in the repo with no local safety documentation. Plan: sample the message-loop/subclassing and any buffer/length-conversion sites first (highest-risk unsafe categories), not a line-by-line pass. |
| 6 | Win32 clipboard ownership transfer | likely `crates/agenterm-platform/src/adapters/windows/clipboard.rs` | Not started | AGENTS.md claims: allocate movable NUL-terminated UTF-16, transfer ownership only after `SetClipboardData` succeeds, free on every pre-transfer failure. Not yet checked against the code (frontend reviewer explicitly flagged this as unreviewed). |
| 7 | AppContainer / process containment unsafe blocks | `crates/agenterm-platform/src/adapters/windows/{app_container,process_containment,process_security}.rs` | Not started | 18–25 unsafe blocks each, no SAFETY comments per initial sweep. Security-sensitive (sandbox token/profile handling); worth a pass after the clipboard file. |
| 8 | Unbounded per-stream drain thread spawn | `src/script_stream.rs` (`from_reader_inner`, ~L122-205) | Not started | Rhai review flagged: no explicit cap on drain threads, bounded only indirectly via upstream process/task concurrency limits. Needs confirming whether that indirect bound is actually reachable for the stream path specifically. |
| 9 | `close_tab` duplicated & already diverged | `src/server_app.rs:1585-1630` vs `src/platform/adapters/unix/frontend/mod.rs:4821-4864` | **Blocked** | File is under active concurrent edit by the other agent working the frontend-parity refactor (`git status` shows it modified). Revisit once that lands — do not touch now. |
| 10 | Sidebar geometry math duplicated (Windows reimplements the shared fn) | `src/ui_geometry.rs:686,803` vs `src/platform/adapters/windows/remote_frontend.rs:1669-1743` | **Blocked** | Same reason as #9 — `remote_frontend.rs` is under active concurrent edit. Revisit once that lands. |

## Done

- #1 `8e1e413` — fork()/execve() async-signal-safety in the POSIX PTY adapter.
