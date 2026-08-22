---
name: windows-binary-portability
description: Make a Windows binary start on older Windows (Server 2016 / Windows 10 1607) and stop depending on the VC++ redistributable. Diagnose "entry point not found" and missing-DLL loader failures, decide which APIs must be resolved with GetProcAddress, and know which CRT linkage changes actually work versus which link but break unwinding. Use when a binary runs on the dev machine but not on the target, when a user reports a symbol-name dialog at startup, when eliminating VCRUNTIME140.dll, or before adding a Win32 API newer than the oldest supported Windows.
---

# Windows binary portability: what the loader demands before `main`

**The one sentence:** on Windows, a dependency problem is not a runtime bug, it
is a *refusal to start*, and the loader tells you about exactly one symbol at a
time — so guessing costs one round trip per symbol, each paid for by whoever
owns the target machine.

## 0. Recognize the failure class

Symptoms that all mean "the PE loader refused the image":

- 「无法定位程序输入点 X 于动态链接库 Y」 / "The procedure entry point X could
  not be located in the dynamic link library Y"
- 「找不到 Y.dll」 / "Y.dll was not found"
- The program "does nothing" when double-clicked, no window, no console output

None of these reach your code. Panic hooks, logging, diagnostics sinks and
crash handlers are all irrelevant: the process image was never started. Do not
try to make the program self-diagnose this.

Every test suite is blind to it too, because CI and dev machines are new enough
to satisfy the import.

## 1. Never guess the next symbol — probe the target once

The loader names one symbol, you fix it, it names the next. Two rounds of that
is a process failure, not bad luck.

Use `scripts/probe-imports.ps1` (in this repo). Run it **on the target
machine**. It parses the PE import table itself — the target has no Visual
Studio, so `dumpbin` is not available there — resolves every named import
against the running system, and prints the complete missing set in one pass.

```
powershell -ExecutionPolicy Bypass -File probe-imports.ps1 -Path .\your.exe
```

If you write a probe like this yourself, it needs two validations or its
all-clear is worthless:

- **Parse validation**: its symbol count must match `dumpbin /IMPORTS` exactly.
  Beware when comparing — dumpbin's *Summary* block lists section names
  (`.text`, `.rdata`, …) in the same column shape as import names and will
  inflate a naive regex count.
- **Probe self-test**: a symbol that cannot exist must fail to resolve, and a
  universal one (`CloseHandle`) must succeed, *before* it prints an all-clear.
  An all-clear is also what a broken probe prints.

PowerShell trap: `0x8000000000000000` (the PE ordinal flag) parses as a signed
`Int64` and overflows the `[uint64]` cast. Use `[uint64]1 -shl 63`.

## 2. A documented minimum version is evidence, not proof

Microsoft's "Minimum supported client" line can be true about the *function*
and false about the *export you link against*.

Real case: `SetThreadDescription` is documented as Windows 10 version 1607.
Windows Server 2016 **is** 1607. It is still missing, because 1607 implements
it only in `KernelBase.dll` and the `kernel32` forwarder did not appear until
1703. SDK header guards (`#if _WIN32_WINNT >= _WIN32_WINNT_WIN10`) do not catch
this either — the guard passes.

Only the target system settles it. See §1.

## 3. Resolve late instead of importing early

For any API newer than your oldest supported Windows, replace the static import
with a run-time lookup. The point is not the lookup, it is *where the failure
lands*: a missing export becomes an ordinary refusal from the one operation
that needed it, instead of a refusal to start.

Shape that works (see `adapters/windows/pty.rs::conpty` and
`adapters/windows/threading.rs::thread_naming`):

- `GetModuleHandleW` for modules already loaded in every process (`kernel32`,
  `KernelBase`, `user32`, `ntdll`) — it borrows the loader's reference, so
  there is nothing to free. `LoadLibraryW` + `FreeLibrary` only for modules
  that may not be loaded.
- Cache in a `OnceLock<Option<Entries>>`. A missing export does not appear
  later in the same process.
- Resolve a related group **all-or-none**. A system exporting half a feature is
  not one you know how to drive, and partial use trades a load-time failure for
  a null call.
- Name the transmute types (`type Create = unsafe extern "system" fn(...)`) —
  clippy's `missing_transmute_annotations` requires it and it documents the ABI
  at the same time.
- Report the *version*, not the symbol. "needs Windows 10 build 17763 (1809)"
  is actionable; "CreatePseudoConsole not found" is not.
- If the capability is cosmetic (thread naming), absence should be a silent
  no-op — do not invent an error a caller must handle.

Known instances in this repo:

| Symbol | Really needs | Note |
|---|---|---|
| `CreatePseudoConsole` / `Resize…` / `Close…` | build 17763 (1809) | ConPTY |
| `SetThreadDescription` | kernel32 forwarder, build 15063 (1703) | documented as 1607, see §2 |

Guard: `crates/agenterm-con/tests/agenterm_con_load_portability.rs` parses the
built PE with the `object` crate and refuses both a blocker symbol and any
module that is neither an OS component nor a recorded exception. Its weakness
is that its blocker list is hand-written — it refuses only what someone already
thought of. It is a supplement to §1, not a replacement.

## 4. UCRT is Windows; VCRUNTIME140 is not

Sort the CRT dependencies before deciding anything:

- `api-ms-win-crt-*.dll` — the **Universal CRT**, an OS component since
  Windows 10 RTM. Present on Server 2016. Leave it dynamic.
- `api-ms-win-core-*.dll` — Win32 API sets, OS components. Fine.
- `VCRUNTIME140.dll` — a **redistributable**. Absent from a clean Server 2016.
  This is the one that needs a decision.

What a Rust binary actually pulls from VCRUNTIME140:

| Symbol | Why |
|---|---|
| `memcpy` `memset` `memmove` `memcmp` | compiler-emitted calls (struct moves, slice copies) |
| `__CxxFrameHandler3` | the MSVC C++ EH personality routine |
| `_CxxThrowException` | how a Rust panic is thrown |

The last two exist because **on `windows-msvc`, Rust's `panic = "unwind"` is
built on MSVC's C++ exception machinery**. Windows itself supplies only the SEH
primitives (`RtlVirtualUnwind`, `RtlUnwindEx` in `ntdll`); the C++ personality
routine is a compiler-runtime concern. LLVM's MSVC ABI has this dependency
hard-coded — it is not a configuration choice.

## 5. Measured results for removing VCRUNTIME140 (agenterm-con, Rust 1.97)

Do not repeat these experiments; extend the table instead.

| Change | Result |
|---|---|
| `-C target-feature=+crt-static` alone | **No effect.** An explicit `cargo:rustc-link-lib=dylib=vcruntime` in `build.rs` overrides it. Check the build script before believing a flag did nothing. |
| `-Z build-std-features=…,compiler-builtins-mem` | Removes `memset` only. The other three still resolve from the CRT import lib first. Does not remove the DLL. |
| `cargo:rustc-link-lib=static=libvcruntime` alone | Links, then dies at `STATUS_STACK_BUFFER_OVERRUN` (0xC0000409). Mixed CRT model: `msvcrt.lib` (dynamic startup) was still linked. |
| Full `static_vcruntime` directive set (below), alone | Links; VCRUNTIME140.dll disappears; `--version` works. But **unwinding is broken** — `catch_unwind` over a panic terminates the process at `STATUS_STACK_BUFFER_OVERRUN`. Necessary, not sufficient. See §6. |
| Same set **+ `__vcrt_initialize()` in the custom entry** | **Adopted.** VCRUNTIME140.dll gone, every remaining module an OS component, `panic = "unwind"` preserved, `caught=true`. PE 740,864 bytes of a 1,048,575 budget (+22,528). Full suite green. |
| Debug CRT names (`libvcruntimed.lib`, `ucrtd.lib`) under a `DEBUG=true` branch | **Wrong — do not add.** Rust links the *release* CRT on windows-msvc in every profile, so the debug names leave `__CxxFrameHandler3` unresolved and `con-dev` stops linking. Use the release set unconditionally. |
| `panic = "abort"` (profile `release-fast`) | Both EH symbols **and all UCRT api-sets** disappear; only the four `mem*` remain. 464,896 bytes vs 717,312 — unwind costs ~35%. **Cost: a panic in any thread kills the process** instead of being contained by `catch_unwind`. Now moot: unwind was kept and the DLL removed anyway. |

The `static_vcruntime` set, for reference (release variants):

```
/NODEFAULTLIB:vcruntime.lib /NODEFAULTLIB:msvcrt.lib /NODEFAULTLIB:libucrt.lib
/DEFAULTLIB:libcmt.lib /DEFAULTLIB:libvcruntime.lib /DEFAULTLIB:ucrt.lib
```

**Current standing decision:** static VC runtime, dynamic UCRT,
`panic = "unwind"`, no redistributable. `vcruntime140.dll` no longer needs to
be shipped.

### How the "unwinding is broken" half was found

Worth reproducing as a method, because the answer was not the first, second, or
third hypothesis:

1. **Bisect the configuration against a minimal binary.** A plain `rustc`
   hello-world with the same six directives unwound fine. Then a cargo project
   with `-Z build-std`: fine. Then with the `.CRT$X*` sentinels: fine. Then
   with con's exact profile chain (`opt-level = "z"`, `strip`, per-package
   `codegen-units`): fine. Four experiments, each one command, each removing a
   suspect. What survives is what is actually different.
2. **Check the assumption you reasoned from.** The reasoning "the test binary
   uses `mainCRTStartup`, so CRT init cannot be the differentiator" was wrong.
   `dumpbin /HEADERS` on both binaries showed **both** entry points are
   `agenterm_con_entry` — Cargo applies `rustc-link-arg-bin=<name>=` to the
   bin's *unittest* binary too, because they share a target name. One command
   turned a dead end into the answer.

## 6. `/ENTRY` must call `__vcrt_initialize` when the VC runtime is static

`__scrt_common_main_seh` — inside the MSVC startup object that `/ENTRY`
replaces — calls `__vcrt_initialize`, which brings up the per-process state
that C++ exception handling needs. **It is not reachable through `.CRT$XI*`**,
so an entry point that walks that table is not doing this.

It stays invisible while the VC runtime is a DLL, because `VCRUNTIME140.dll`
initializes itself from `DllMain` at load. Link it statically and there is no
`DllMain`, nobody initializes it, and the first panic dies at
`STATUS_STACK_BUFFER_OVERRUN` — a failure that reads like stack corruption and
is really a missing constructor.

```rust
unsafe extern "C" { fn __vcrt_initialize() -> i32; }

pub extern "system" fn my_entry() -> ! {
    if unsafe { __vcrt_initialize() } == 0 { unsafe { ExitProcess(254) } }
    // ... then the .CRT$XI / .CRT$XC walk, then main
}
```

Before the `.CRT$XI*` walk: those initializers may themselves unwind.

## 7. `/ENTRY` and the security cookie — measured, not assumed

Microsoft documents that a program defining its own entry point with `/ENTRY`
must call `__security_init_cookie()` itself, because the MSVC startup object
that normally does it never runs. `__security_cookie` is compared by every
`/GS`-protected function on return and by exception handling; left at its fixed
default `0x00002B992DDFA232`, an ordinary return can report as a buffer
overrun — `STATUS_STACK_BUFFER_OVERRUN` with no usable stack.

**In this repo the call is already covered and must not be added.** Measured
by reading `__security_cookie` from the product binary:

| Build | Cookie |
|---|---|
| with an explicit `__security_init_cookie()` in `agenterm_con_entry` | `0x0000ad2ba86ad468` — random |
| **without it (negative control)** | `0x0000382610e9972b` — **also random** |

The reason: `startup.rs` walks `.CRT$XI*`, and the CRT registers its cookie
initializer in that table. The custom entry therefore already runs it. Adding
an explicit call is redundant and mildly harmful — a second initialization
shifts the cookie under frames that already stamped the first value.

The general lesson is the point: **the documented requirement was true and the
fix was still wrong.** Run the negative control before committing a change
justified by documentation rather than by measurement. Reading the cookie is
cheap:

```rust
unsafe extern "C" { static __security_cookie: usize; }
let value = unsafe { std::ptr::read_volatile(&raw const __security_cookie) };
```

## 8. Probing unwinding

Unwinding cannot be verified by "the binary runs" — `--version` succeeds in a
build whose `catch_unwind` is broken. Add a temporary argument to the **product
binary** (not a test binary, whose link line differs) and compare against a
known-good baseline:

```rust
if args.iter().any(|a| a == "--probe-unwind") {
    let caught = std::panic::catch_unwind(|| panic!("unwind probe")).is_err();
    eprintln!("unwind probe: caught={caught}");
    std::process::exit(0);
}
```

Working: prints `caught=true`, exit 0. Broken: the panic message prints and the
process dies without the line. **Always run the baseline too** — a probe that
never prints `caught=true` anywhere is measuring itself.

## 9. Build-configuration traps that cost real time

- **`cargo test` relinks the binary differently from `cargo build`.**
  Dev-dependencies unify features across the graph, so the `agenterm-con.exe`
  left in `target/<profile>/` after a test run is *not* the shipping binary.
  Always re-run `cargo build` before copying to `dist/`.
- `cargo:rustc-link-lib=` and plain `cargo:rustc-link-arg=` apply to **every**
  target of the crate, including test binaries.
  `cargo:rustc-link-arg-bin=<name>=` applies only to that bin — which is why
  `/ENTRY` reaches the product binary but not the test binaries, and why the
  two can fail in different ways.
- `NoDefaultCurrentDirectoryInExePath=1` in a session shell breaks `mlua-sys`
  and other build-script rebuilds. `unset` it first.
- Renaming a running `.exe` is allowed on Windows. Use it to stage a new build
  over a locked path without killing the user's session.
- A running GUI instance holds `target/.../agenterm-con.exe` and makes cargo
  fail with `os error 5`. Sweep before building.

## 10. Reporting rule

Say what was verified on which machine. "Import-table derivation plus
equivalent local verification" is not "tested on Server 2016". The distinction
is the whole reason §1 exists.
