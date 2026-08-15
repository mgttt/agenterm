# Goal: first-class `agenterm-dyn` on macOS (wave 2)

Status: active  
Owner: this session (`agenterm-osx`) + exclusive-file subagents **in background**  
CWD: repository root. Paths: repo-relative or `~/...` only.  
Git identity for this clone: `agenterm-osx <agenterm@mgttt.com>`.

## Outcome

`agenterm-dyn` stays a tiny intern/eval/`dlcall` door. Wave 1 on `main` already
has 40-slot probes, Darwin-only live rows, and a signature-gated ioctl path.
Wave 2 makes that path honest (loaded symbol, not a linked `ioctl` bypass),
adds more Darwin facts that are not Linux clones, and stops the leftover
“maybe −1” smoke wording.

Evidence: `cargo test -p agenterm-dyn` on this aarch64-apple-darwin host.

## Invariants

- No C source, no libffi, no JIT, no lambda/cons/strings, no cu/platform import.
- OS names stay **script data**. Host tables stay six-cell explicit.
- Restore process-global side effects before a test ends (`umask` pattern).
- Do not fake a live probe on the wrong OS.
- Subagents do **not** commit or push. Primary owns I/P.
- Commit-pull-rebase-push on each coherent increment. Exact pathspec. No `git add -A`.
- Subagents run **in background**. Exclusive write sets. Private
  `CARGO_TARGET_DIR` (`target/dyn-ioctl2`, `target/dyn-probes2`, `target/dyn-smoke`).

## Current facts (do not rediscover)

- `system_probes` is `[SystemProbe; 40]`. First 36 Linux/macOS live; last 4
  Darwin-only live on macOS (`sysctlbyname`, `mach_absolute_time`,
  `getprogname`, `issetugid`); Linux last 4 + all Windows are Placeholder.
- Header aliases including `off_t` / `mode_t` / `pid_t` / `uid_t` / `gid_t` /
  `time_t` / `socklen_t` / `nfds_t` already reject.
- Darwin ioctl: `tests/macos_ioctl.rs` claims TIOCGWINSZ success. `native.rs`
  currently calls a **linked** `extern "C" { fn ioctl(...) }` and ignores the
  `libloading` pointer. That is a bypass, not `dlcall`.
- `tests/smoke.rs` macos `dlcall_ioctl_winsize` still says “success is not
  claimed”. README test-suite paragraph is stale the same way.
- `getloadavg` uses `double[]` — **out of ABI**. Do not add it.

## DAG

```text
G  this goal (wave 2)
├── A  ioctl via loaded symbol     files: src/native.rs only
├── B  more Darwin-only probes     files: src/hosts.rs, tests/hosts.rs,
│                                  tests/macos_probes.rs (append),
│                                  examples/{nsget-executable-path,
│                                  proc-pidpath,arc4random}.md (new)
├── C  leftover honesty            files: tests/smoke.rs (macos ioctl
│                                  test + comment only),
│                                  examples/ioctl-window-size.md
└── I  integrate + README/PRD/cheatsheet (primary)
    └── P  commit / pull --rebase / push
```

Do not edit README / PRD / cheatsheet in A/B/C. Primary owns those after
leaves land. Do not grow general variadic FFI.

## Leaves

### A — ioctl is still a `dlcall`

Replace the linked `extern "C" { fn ioctl(...) }` with a transmute of the
already-resolved `func_ptr`:

```rust
type IoctlFn = unsafe extern "C" fn(i32, u64, ...) -> i32;
let f: IoctlFn = transmute(func_ptr);
f(fd, request, ptr)
```

Keep the signature gate. Keep Linux invoke unchanged. `tests/macos_ioctl.rs`
must still pass (do not edit it). If the loaded-symbol path EFAULTs, **stop**
and leave the linked declaration with a one-line comment that the bypass
remains; do not add a C file.

### B — three more Darwin facts

Grow `system_probes` 40 → 43 on **all** six cells. Append, same order
everywhere:

| name | Darwin | Linux / Windows |
|------|--------|-----------------|
| `nsget_executable_path` | live `libSystem.B.dylib` / `_NSGetExecutablePath` | Placeholder |
| `proc_pidpath` | live `libSystem.B.dylib` / `proc_pidpath` | Placeholder |
| `arc4random` | live `libSystem.B.dylib` / `arc4random` | Placeholder |

Append-only tests in `tests/macos_probes.rs`:

- `_NSGetExecutablePath`: caller-owned buffer + `u32` length; rc 0; path
  non-empty; agrees with a second libc/`std::env::current_exe` prefix.
- `proc_pidpath(getpid(), buf, len)`: rc > 0; buffer C string non-empty.
- `arc4random`: `"u32"` return; two calls both fit `i64`; no crash.

One new example md per name. If a libc symbol is missing, **stop** that row
and leave it out of the table — do not fake live.

### C — stop saying “maybe −1”

In `tests/smoke.rs` macos `dlcall_ioctl_winsize`: when `openpty` slave
succeeds, assert `code == 0` and 24×80. Keep a fallback fd path honest if
openpty fails. Update the comment. Do not rewrite other smoke tests.

In `examples/ioctl-window-size.md`: add a second lisp block for
`libSystem.B.dylib` + macOS `TIOCGWINSZ` `0x40087468`. Keep the Linux block.

## I / P

Primary: rustfmt, `cargo test -p agenterm-dyn`, README + PRD 34 + cheatsheet
alignment, exact-path commit as `agenterm-osx`, rebase `origin/main`, push.
Repeat whenever a leaf is coherent.

## Non-goals

Windows live rows. JIT. cu/platform wiring. libagenterm merge. General
variadic FFI. `getloadavg`. Editing files outside the assigned set.
