# agenterm-dyn Grok review (Windows follow-up)

Date: 2026-08-15. Reviewer: Grok session on Linux (no Windows host in this
session). Status: review + handoff only; **not implemented**.

Scope: `crates/agenterm-dyn` as of this write, with **Windows OS** as the
primary surface. Linux / macOS are context only. Intended reader: a later
agent sitting on **real Windows** (`x86_64-pc-windows-msvc` first;
`aarch64-pc-windows-msvc` compile-only unless a native ARM64 box exists).

Do not treat [`plan/agenterm-dyn-cc-review.md`](agenterm-dyn-cc-review.md) as
current for CI or macOS. That note predates live Darwin `system_probes` and
the dyn steps already in `.github/workflows/ci-agenterm.yml`.

Product bar: [`prd/PRD_02_34_agenterm_dyn.md`](../prd/PRD_02_34_agenterm_dyn.md).
Windows extra probes stay **placeholders** until 政委 flips that sentence.
This file is the Windows verification / fill-in playbook, not a license to
thicken the crate.

## 1. What is true on Windows today

| Item | Fact |
|------|------|
| Host rows | `WINDOWS_X86_64` and `WINDOWS_AARCH64` in `crates/agenterm-dyn/src/hosts.rs` |
| PID | `kernel32.dll` / `GetCurrentProcessId` / `u32` |
| Secondary | `kernel32.dll` / `GetCurrentThreadId` / `u32` |
| Size probe (data only) | `GetConsoleScreenBufferInfo` — **named, never executed** |
| Extra `system_probes` | 36 Linux-named rows, all `Placeholder` (noise: `umask`, `fcntl`, `nice`, …) |
| Live smoke | `tests/smoke.rs` `mod windows`: PID, TID, CRT `getenv` via `ucrtbase.dll` then `msvcrt.dll` |
| Missing-symbol / `do` / size / ptr-out | **not** in the Windows module (Linux/macOS have them) |
| Examples | Unix-only S-exprs. `examples/getpid.md` mentions the Win names in prose only |
| `windows-sys` features | `Win32_System_Threading` only (enough for the two ID APIs) |
| Root binary / cu / platform | still not wired (correct per PRD) |

### CI (already better than the CC review)

`.github/workflows/ci-agenterm.yml`:

- Ubuntu `quality` job: `cargo test --locked -p agenterm-dyn` (Linux live suite).
- `win-x86_64` (`windows-latest`, native): `cargo test --locked -p agenterm-dyn --target x86_64-pc-windows-msvc`.
- `osx-aarch64` / `osx-x86_64`: same native test step.
- `win-aarch64` (`cargo-xwin` on Ubuntu): `cargo xwin check --locked -p agenterm-dyn --all-targets`.
- `lnx-aarch64`: compile-only check.

So the three Windows smokes **are compiled and run** on GitHub `windows-latest`.
They have never been re-checked in *this* session on a real desktop (RDP,
Windows Terminal, redirected `cargo test`, no-console service session). That
is the first job of the Windows agent.

`extern "C"` trampoline in `native.rs` is acceptable on both 64-bit Windows
targets: MSVC x64 and Windows ARM64 use the same convention for `"C"` and
`"system"`. 32-bit `stdcall` / `_Foo@4` is **not a target**.

## 2. Windows ABI landmines (read before writing probes)

These are the reasons a Linux-shaped probe table will lie or crash on Windows.

### 2.1 Type spellings

`dlcall` accepts only `void` / `i8` `u8` `i16` `u16` `i32` `u32` `i64` `u64` /
`ptr`. Win32 names (`DWORD`, `HANDLE`, `BOOL`, `LPCWSTR`, `ULONG_PTR`) reject
before load. Map them:

| Win32 | dlcall |
|-------|--------|
| `BOOL` / `DWORD` / `UINT` | `i32` or `u32` (see high-bit rule) |
| `HANDLE` / `HMODULE` / `HWND` / `HDC` / `HHEAP` | **`ptr`**, never `u32` / `u64` |
| `void` stdcall | `void` |
| pointer out-param | `ptr` + `Dyn::bind` |

`u64` returns that do not fit in `i64` error (`native.rs`). Handle/address
returns must be `ptr`. `GetTickCount64` as `u64` is fine for any realistic
uptime; still prefer not to treat it as a handle.

### 2.2 High-bit constants cannot be `i32`

Parser has **no hex** (`0x…` is a parse error). S-expr integers are decimal
`i64`. `i32` rejects any value outside `[-2147483648, 2147483647]`.

| Constant | Decimal | Slot |
|----------|---------|------|
| `STD_OUTPUT_HANDLE` (−11) | `-11` | **`i32`** (or `u32` `4294967285`) |
| `STD_INPUT_HANDLE` | `-10` | `i32` |
| `STD_ERROR_HANDLE` | `-12` | `i32` |
| `GENERIC_READ` `0x80000000` | `2147483648` | **`u32` only** — `i32` overflows |
| `GENERIC_WRITE` | `1073741824` | `u32` (or `i32`) |
| `GENERIC_READ\|GENERIC_WRITE` | `3221225472` | **`u32` only** |
| `INVALID_HANDLE_VALUE` (−1 as handle) | cannot pass as `ptr` via negative `Int` | bind from Rust, or compare returned `ptr` to `usize::MAX` |

`Value::as_ptr` accepts non-negative `Int` only. `"ptr" 0` is NULL.
`"ptr" -1` is a type error.

### 2.3 Backslash is not a string character

`parse.rs`: a `\` inside `"…"` is **parse failure** (“escape sequences in
strings are not supported yet”). You **cannot** write
`"C:\Windows\System32\kernel32.dll"` in an S-expr.

Use search-order names (`kernel32.dll`, `user32.dll`, `ucrtbase.dll`) or
forward-slash full paths (`C:/Windows/System32/kernel32.dll`). Prefer the
bare system DLL name. `LibraryCache` keys are case-sensitive strings even
though Win32 load is not — `"kernel32.dll"` vs `"Kernel32.dll"` are two
cache entries.

### 2.4 `GetLastError` is almost useless as a second `dlcall`

`GetLastError` is thread-local and is clobbered by almost any later Win32
call. `eval` / `HashMap` / `libloading` after the failing call can change it.
Do **not** assert `GetLastError` from a separate `Dyn::eval` after
`GetConsoleScreenBufferInfo` failed. Either:

- bind a small Rust helper that reads `GetLastError` immediately (out of
  scope for script-only probes), or
- treat BOOL `0` as the honest failure and skip the error code, or
- call `SetLastError` then `GetLastError` in one `(do …)` **only** as a
  trampoline smoke, knowing eval between the two forms may still clobber.

`SetLastError` is a process/thread side effect. Restore the previous value
before the test ends (same rule as Linux `umask`).

### 2.5 Console geometry is not `ioctl`

`GetConsoleScreenBufferInfo(h, &info)` returns `BOOL`. On GitHub
`windows-latest` and on `cargo test` with redirected stdout it usually
returns `0` — that is **honest**, not a trampoline bug. Do not claim
rows/cols unless you created a buffer you control.

Headless recipe that still exercises the named size probe (≤6 args):

1. `CreateConsoleScreenBuffer` (5 args) → `ptr`
2. `GetConsoleScreenBufferInfo` on that handle + bound
   `CONSOLE_SCREEN_BUFFER_INFO`
3. `CloseHandle` the buffer (restore / no leak)

`CreateConsoleScreenBuffer` desired-access uses `GENERIC_*` — must be `u32`.
`CONSOLE_TEXTMODE_BUFFER` is `1`.

`AllocConsole` / `AttachConsole` / `FreeConsole` change process-global
console state. Do not use them in the crate test unless you restore and
document the session impact. Prefer the screen-buffer handle.

Windows Terminal / ConPTY / RDP / Cairo-without-console: GCSBI on
`GetStdHandle(-11)` is optional and may fail. Record the BOOL; do not fake
24×80.

### 2.6 Arity 6 kills common Win32

`CreateFileW` is **7** arguments. `CreateProcessW` is 10. `FormatMessageW`
is 7. They cannot go through `dlcall`. Do not “almost” call them.

Fits: 0–6 integer/pointer args, fixed, non-variadic. Win32 is rarely
variadic; the Darwin `ioctl` problem is not the Windows default.

### 2.7 Wide vs ANSI, and no strings in the language

There is no string value type. `LPCWSTR` buffers are UTF-16 units bound from
Rust (`Vec<u16>` + NUL). Length args for `*W` APIs are **WCHAR counts**, not
bytes (`GetCurrentDirectoryW`, `GetComputerNameW`, `GetEnvironmentVariableW`).

`A` APIs take bound `CString`. Do not mix a pointer from `ucrtbase.dll`
`getenv` with `msvcrt.dll` `free` (dyn never frees — keep it that way).

### 2.8 Callbacks and COM are not this door

CU-adjacent catalog rows are **script data only**:

- `user32.dll` / `EnumWindows` needs a `WNDENUMPROC`. You cannot mint a
  callback from S-expr. Do not live-call it unless Rust binds a real
  callback pointer (that is already cu’s job).
- `UIAutomationCore.dll` is COM (`CoCreateInstance` + vtable). Not a
  `dlcall` smoke.

Same rule as “no cu / platform import”: naming a fact is not executing it.

### 2.9 What this crate is allowed to be

No libffi, no C shim, no JIT, no fourth engine, no `agenterm-platform` /
`agenterm-cu` import. OS names stay opaque at the eval boundary. Host table
rows are `PLATFORM-CANDIDATE` data.

## 3. Issues (Windows-weighted)

### Issue 1 — Severity: bug (contract / honesty)

- File: `crates/agenterm-dyn/tests/smoke.rs` (windows module, ~1621)
- Description: Size probe `GetConsoleScreenBufferInfo` is first-class host
  data and README “six-cell” copy, but Windows never `dlcall`s it. The live
  suite only proves two ID APIs + optional CRT getenv. A green
  `win-x86_64` CI job does **not** prove the documented console door.
- Suggestion: On a real Windows box, add one smoke that **resolves** GCSBI
  (and ideally writes a caller-owned `CONSOLE_SCREEN_BUFFER_INFO` via a
  `CreateConsoleScreenBuffer` handle). Success of geometry is optional;
  symbol resolve + BOOL 0/1 is the Darwin-ioctl honesty bar.
- Status: open — needs Windows host.

### Issue 2 — Severity: bug (table shape)

- File: `crates/agenterm-dyn/src/hosts.rs` (`PLACEHOLDER_SYSTEM_PROBES`,
  `WINDOWS_*` rows)
- Description: Windows is forced to wear 36 Linux probe names. Tests in
  `tests/hosts.rs` assert every Windows extra row is `Placeholder`. Filling
  live Win32 without changing the array shape would either lie (`getuid` →
  some kernel32 symbol) or keep the noise forever.
- Suggestion: Prerequisite to any real Windows probe fill: change
  `system_probes: [SystemProbe; 36]` to a slice / per-OS array with **Win32
  names**. That is the CC review step 2. It needs 政委 if it is framed as
  “leave Win placeholders” vs “rename the placeholder rows”. Safer
  incremental path: keep the 36 Linux names as `Placeholder` and add a
  **separate** `windows_probes` field — but that thickens `HostCell`. Prefer
  slice + OS-specific names once authorized.
- Status: open — policy + shape.

### Issue 3 — Severity: suggestion (coverage gap vs Linux/macOS)

- File: `crates/agenterm-dyn/tests/smoke.rs` windows module
- Description: No missing-symbol-does-not-evict-cache test, no `(do (set
  pid …))` test, no caller-owned ptr-out test. Those are the bugs that show
  up only on `libloading` + PE (`GetProcAddress`, forwarded exports).
- Suggestion: Port the three Linux tests 1:1 with kernel32 names before
  adding new APIs. Cheap, PRD-compatible (does not flip placeholder rows).
- Status: open — Windows agent can do this without a PRD change.

### Issue 4 — Severity: suggestion (CRT getenv is a side path)

- File: `crates/agenterm-dyn/tests/smoke.rs` (`ucrtbase.dll` / `msvcrt.dll`)
- Description: Fallback is hand-written in the test, not host-table data.
  `DISPLAY` is usually unset on Windows; the test only requires “runs”.
  `ucrtbase.dll` is present on Win10+ CI images; older / exotic SKUs may
  only have `msvcrt.dll`.
- Suggestion: If/when the table grows, list CRT candidates as table data
  (CC review step 5). Until then, keep the match, and add
  `GetEnvironmentVariableA` / `W` on `kernel32.dll` as the Win32-native
  equivalent (3 args, bound buffer) — that is a better “getenv” than CRT.
- Status: open.

### Issue 5 — Severity: nit (docs vs CI)

- File: `crates/agenterm-dyn/README.md` (Windows test paragraph)
- Description: README still sounds like Windows is “local / CI when
  available”. CI now **always** runs the three smokes on `windows-latest`.
- Suggestion: Say that `win-x86_64` CI runs PID/TID/getenv; extra probes
  and GCSBI remain unverified live.
- Status: open.

### Issue 6 — Severity: nit (examples)

- File: `crates/agenterm-dyn/examples/`
- Description: Every example is libc S-expr. PRD “pair every new live
  probe” will fail on Windows unless new `examples/*.md` use `kernel32.dll`
  and avoid backslash paths.
- Suggestion: First Windows example should be PID (`GetCurrentProcessId` /
  `u32`), then one ptr-out (`GetSystemTimeAsFileTime` or GCSBI). No cu
  wiring in the prose.
- Status: open — only when a probe is live.

## 4. Follow-up for the Windows agent (do in this order)

Work from the repository root. Exclusive file domains below. Do not use a
worktree unless 政委 says so. Do not import `agenterm-platform` /
`agenterm-cu`. Do not edit Linux/macOS smoke modules. No hex in S-exprs.
No `\` in S-expr strings. No absolute account paths in anything you commit
(`~/…` or repo-relative; run `./scripts/doc-redact-check.sh` on md you
touch — on Windows use Git Bash or the equivalent from `scripts/`).

### Phase A — prove the door (no PRD change)

File domain: `crates/agenterm-dyn/tests/smoke.rs` (windows module only).

1. `cargo test --locked -p agenterm-dyn`
   Expect language + hosts + errors + the three Windows smokes green.
2. Record (in the later evidence comment / this file’s “Evidence” section,
   not in chat-only notes):
   - `target` triple (`x86_64-pc-windows-msvc` vs ARM64)
   - whether stdout is a console (`GetConsoleMode` on STD_OUTPUT)
   - Windows Terminal / conhost / redirected CI
   - `GetCurrentProcessId` dlcall == `windows-sys` / `GetCurrentProcessId`
3. Add, still against **existing** live cells only:
   - missing symbol on `kernel32.dll` does not evict the cached library
     (then PID still matches)
   - `(do (set pid (dlcall … GetCurrentProcessId …)) pid)`
   - optional: `GetStdHandle` + `i32` `-11` → `ptr` (NULL / invalid is
     allowed; must not type-error)
4. Expand `windows-sys` features in `crates/agenterm-dyn/Cargo.toml` only
   when a test needs a typed struct / constant for a **cross-check**. Keep
   features minimal (`Win32_System_Console`, `Win32_Foundation`,
   `Win32_System_SystemInformation`, …).

Pass bar: same three original tests plus the cache/`do` ports. No new
`SystemProbe` rows.

### Phase B — honest size probe (still compatible with “placeholders”)

File domain: `tests/smoke.rs` windows module + `Cargo.toml` windows-sys
features. Do **not** flip `PLACEHOLDER_SYSTEM_PROBES`.

1. Bind a zeroed `CONSOLE_SCREEN_BUFFER_INFO` (`#[repr(C)]` matching
   Win32: `COORD` ×2, `u16` attributes, `SMALL_RECT`, `COORD` max).
2. Prefer `CreateConsoleScreenBuffer` → GCSBI → `CloseHandle`.
3. Assert: dlcall resolves; BOOL is `0` or `1`. If `1`, `dwSize.X/Y` > 0.
4. If `CreateConsoleScreenBuffer` is too much for the first cut: call GCSBI
   on `GetStdHandle(-11)` and accept BOOL `0` on redirected output.

This implements the **named** `SizeProbe` without claiming the 36 extra
rows are live.

### Phase C — live Win32 extra probes (needs PRD + table shape)

Do **not** start this until 政委 replaces “Win six-cell extra probes stay
placeholders” and agrees `system_probes` may become a slice of **Windows
names**.

Then fill only trampoline-legal, restore-safe APIs. Suggested first pack
(mirrors Linux integer / void / caller-owned ptr, not POSIX names):

| name | lib | symbol | ret | args | notes |
|------|-----|--------|-----|------|-------|
| `get_tick_count64` | `kernel32.dll` | `GetTickCount64` | `u64` | — | monotonic ms; compare Δ with a later call |
| `switch_to_thread` | `kernel32.dll` | `SwitchToThread` | `i32` | — | yield analog |
| `sleep_zero` | `kernel32.dll` | `Sleep` | `void` | `u32` 0 | void path; do not sleep >0 in tests |
| `get_acp` | `kernel32.dll` | `GetACP` | `u32` | — | >0 |
| `get_process_heap` | `kernel32.dll` | `GetProcessHeap` | `ptr` | — | non-null |
| `get_module_handle_null` | `kernel32.dll` | `GetModuleHandleW` | `ptr` | `ptr` 0 | exe HMODULE |
| `get_system_time_as_filetime` | `kernel32.dll` | `GetSystemTimeAsFileTime` | `void` | `ptr` FILETIME | caller-owned 8 bytes |
| `query_performance_counter` | `kernel32.dll` | `QueryPerformanceCounter` | `i32` | `ptr` i64 | BOOL 1; value increases |
| `query_performance_frequency` | `kernel32.dll` | `QueryPerformanceFrequency` | `i32` | `ptr` i64 | BOOL 1; >0 |
| `get_system_info` | `kernel32.dll` | `GetSystemInfo` | `void` | `ptr` SYSTEM_INFO | `dwPageSize` >0 |
| `get_current_directory_w` | `kernel32.dll` | `GetCurrentDirectoryW` | `u32` | `u32` wchar cap, `ptr` buf | compare to `std::env::current_dir` via lossy/wide |
| `set_last_error_roundtrip` | `kernel32.dll` | `SetLastError` + `GetLastError` | — | restore previous | see §2.4; do not over-claim |

Pair each new **live** probe with `examples/<name>.md` and a README link.
Restore every process-global write.

Explicitly **out** of the first pack: `CreateFileW` (arity 7),
`CreateProcessW`, `EnumWindows`, UIA, `ntdll` / `Nt*`, `MessageBox*`,
anything that needs a callback or a security descriptor you do not own.

### Phase D — CI / docs only if Phase B–C land

`win-x86_64` already runs tests. `win-aarch64` stays xwin **check** unless
you have a native ARM64 Windows runner. Do not add a C shim to make xwin
“more native”.

README: replace “CI when available” with the actual matrix (Phase A/B
evidence).

## 5. Local Windows commands

From the repository root, MSVC toolset (not `*-windows-gnu`):

```text
cargo test --locked -p agenterm-dyn
cargo test --locked -p agenterm-dyn -- --nocapture
```

`check.cmd` / `build.bat` bootstrap the **workbench**, not this crate.
Do not run the full GUI product to validate dyn.

If `os error 5` appears on an unrelated `agenterm.exe` relink, that is the
locked-PE issue in the Windows GUI skill — irrelevant to `agenterm-dyn`
unless you accidentally built `-p agenterm`.

## 6. Evidence the Windows agent must paste back

Desensitize: no account, no real host, no drive-letter home paths.
Use `~/…` and repo-relative paths.

- Triple and `live_cell()` = `WINDOWS_X86_64` or `WINDOWS_AARCH64`
- `cargo test --locked -p agenterm-dyn` summary line
- For each new smoke: S-expr, declared types, BOOL/int/ptr result class,
  whether a console existed
- Any `Library(...)` / `DlCall(...)` error text (DLL name + Win32 code)
- Confirmation that `CreateFileW` / `EnumWindows` / UIA were **not** called
- If Phase C started: quote the PRD sentence that authorized leaving
  placeholders

## 7. Non-goals (repeat for the assignee)

- No JIT / sljit / DynASM / libffi / C files.
- No lambda / cons / string values / quote / hex / escapes.
- No merge into libagenterm or the root `agenterm` binary.
- No “fix” via `explorer.exe`, and no GUI launch — this crate has no window.
- No 32-bit Windows.

## 8. Verdict

The intern / parse / `dlcall` trampoline is small enough that **64-bit
Windows can be a first-class live cell** without new machinery. What is
missing is not an ABI rewrite; it is Windows-shaped script data, honest
console/ptr smokes, and a table that stops pretending Win32 is POSIX.

Until Phase C is authorized, the Windows agent should stop at **Phase A+B**:
prove cache/`do`/GCSBI on a real box, keep the 36 extra rows as
placeholders, and bring back evidence. That is the highest-value work this
review can assign.
