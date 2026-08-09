# agenterm-guestcore

Phase 1 of [`plan/design-dynacore-emulated-guest-core.md`](../../plan/design-dynacore-emulated-guest-core.md)
(§2 "Phase 1"): a real x86_64 machine-code interpreter, running on Windows,
that decodes real x86_64 opcodes from a byte array and translates a useful
subset of the Linux x86_64 syscall ABI to real Win32 calls via
`agenterm_nativecore::seam::do_intent`. This is **not** a custom IR, not
FleetCall, not a bytecode format -- the guest bytes are genuine x86_64
machine code (hand-encoded for this round's test programs; nothing on this
box could assemble a `.s` file directly, see "How the test programs were
built" below). No JIT, no codegen, no executable memory is ever requested;
guest bytes are read as data and switched on, never executed by the host
CPU.

Same-ISA only (x86_64 guest on x86_64 host) -- this round deliberately does
not attempt aarch64 decoding (Phase 2, not built yet).

> A wider "cover all N ISA/OS combinations now" direction was raised while
> this crate was being built. That is explicitly **out of scope for this
> round** by the design doc's own staging (§2: Phase 1 proves the mechanism
> on one ISA/OS pair first; Phase 2 is aarch64; Phase 3+ is more syscall/
> container coverage) and by this round's own dispatch instructions
> ("do not attempt aarch64 decoding, that's explicitly out of scope this
> round"). It is flagged here for whoever plans Phase 2+, not silently
> dropped.

## Three-layer structure

Kept deliberately separate so a Phase 2 aarch64 decoder is an *additive*
sibling module, not a rewrite of this one:

1. **[`src/decode_x86_64.rs`](src/decode_x86_64.rs) -- the ISA layer.** Pure
   x86_64 semantics: fetch/decode/execute one instruction at a time against
   a generic 16-register file (`src/cpu.rs`). Knows nothing about Linux
   syscall numbers or Windows. When it decodes the guest `syscall`
   instruction (`0F 05`), it does not read `rax`/`rdi`/etc. itself -- it
   just reports "a syscall trap happened" and hands control back with the
   current register file intact.
2. **[`src/abi_linux_x86_64.rs`](src/abi_linux_x86_64.rs) -- the guest-OS ABI
   layer.** Knows exactly one thing layer 1 deliberately doesn't: that this
   guest program was compiled against the Linux x86_64 syscall convention
   (`rax` = number, args in `rdi, rsi, rdx, r10, r8, r9`). Turns a raw
   register snapshot into `GuestSyscall`, a small ISA-agnostic request enum.
3. **[`src/intent_map.rs`](src/intent_map.rs) -- the host-OS mapping layer.**
   `GuestSyscall` -> `agenterm_nativecore::ir::Intent` + real Win32 calls.
   Never reads a guest register or knows an x86_64 opcode exists.

[`src/lib.rs`](src/lib.rs) is the concrete x86_64+Linux+Win32 assembly point
(`run_guest`) that wires the three layers together for real. A future
`decode_aarch64.rs` + `abi_linux_aarch64.rs` (x8=nr, x0..x5=args) would
produce the exact same `GuestSyscall` enum from a different register
convention -- layer 3 and the nativecore backend would not need to change.

Memory (`src/mem.rs`) and addressing (`src/decode.rs`) are shared, ISA-level
concerns available to layer 1.

## Opcode coverage

REX prefixes only (`40`-`4F`, W/R/X/B); no `66`/`67`/`F2`/`F3` legacy
prefixes (see "Not supported" below).

| Category | Opcodes |
|---|---|
| Data movement | `B8`-`BF` mov r32/r64,imm32/imm64 · `C7 /0` mov rm,imm32 · `89` mov rm,r · `8B` mov r,rm · `8D` lea r,m |
| Extended movement | `0F B6`/`B7` movzx (rm8/rm16) · `0F BE`/`BF` movsx (rm8/rm16) |
| ALU rm,r | `01` add · `09` or · `21` and · `29` sub · `31` xor · `39` cmp |
| ALU r,rm | `03` add · `0B` or · `23` and · `2B` sub · `33` xor · `3B` cmp |
| ALU imm | `81 /digit` imm32 · `83 /digit` imm8 (sign-extended); digit: 0 add, 1 or, 4 and, 5 sub, 6 xor, 7 cmp |
| Test | `85` test rm,r |
| Stack | `50`-`57` push r64 · `58`-`5F` pop r64 |
| Control flow | `E8` call rel32 · `C3` ret · `E9` jmp rel32 · `EB` jmp rel8 · `70`-`7F` jcc rel8 · `0F 80`-`8F` jcc rel32 |
| Other | `90` nop · `0F 05` syscall |

Full ModRM + SIB + disp8/disp32 + RIP-relative addressing (`src/decode.rs`),
not just the two or three forms the `guest_interp.rs` reference prototype
used. Condition codes implemented: `o/no/b/ae/e/ne/be/a/s/ns/l/ge/le/g`
(parity-flag conditions `p`/`np` are not modeled -- no test program in this
round needs them, and no ALU op here computes a parity flag).

### Not supported (opcodes)

- `66`/`67`/`F2`/`F3` legacy prefixes (16-bit operand size, address-size
  override, SSE scalar forms) -- any guest byte stream using them is an
  honest `UnimplementedOpcode` fault, not a silent misdecode.
- `adc`/`sbb` (ALU digits 2/3) -- rejected explicitly even though the ALU
  imm/rm dispatch could trivially "guess" a shape for them; flags carry
  input (`CF`) is not modeled.
- 8-bit `mov` (`88`/`8A`/`C6`), `inc`/`dec`/`FF`-group, `test`/`not`/`neg`/
  `mul`/`imul`/`div`/`idiv` (`F6`/`F7`-group), shifts (`C0`/`C1`/`D0`-`D3`),
  `xchg`, string ops, any SSE/SIMD form. None of this round's verification
  programs need them; ape-vm's README documents them for a fuller x86_64
  guest corpus this crate does not target yet.

## Syscall mapping

Linux x86_64 syscall convention: `rax` = number, args in
`rdi, rsi, rdx, r10, r8, r9`. `agenterm_nativecore::ir::Intent::contract_arity`
documents each Intent's own arg order -- generally **not** the same order as
the matching Linux syscall's args. The real translation this crate
implements:

| Guest syscall | nativecore `Intent` | Real arg-order translation |
|---|---|---|
| `write(fd, buf, len)`, **only `fd == 1`** | `WriteStdout` | `[buf, len]` (fd dropped -- `WriteStdout` is hardcoded to `GetStdHandle(STD_OUTPUT_HANDLE)`, it does not take a handle argument at all) |
| `open`/`openat`, **read-mode flags only** (no `O_WRONLY`/`O_RDWR`/`O_CREAT`/`O_TRUNC`) | `FileOpen` | `[path_ptr]` (path only -- `dirfd`/`mode` args are read out of the register file but not passed through; `dirfd` is always treated as "resolve `path` as given", there is no relative-to-dirfd resolution) |
| `read(fd, buf, count)` | `FileRead` | `[handle, buf, cap]` -- **`handle` comes from this crate's own fd table, not the guest's `fd` number.** See "The fd table" below. |
| `close(fd)` | `FileClose` | `[handle]` -- same fd-table indirection |
| `mmap(addr, len, prot, flags, fd, off)`, **only `addr == 0` and `MAP_ANONYMOUS` set** | `Alloc` | `[len]` (`prot`, `fd`, `off` are read but not passed through -- `Alloc` is a fixed `VirtualAlloc(..., MEM_COMMIT, PAGE_READWRITE)`, it does not model a `prot`/file-backing choice) |
| `exit(code)` / `exit_group(code)` | *(none -- process-level)* | direct real `ExitProcess(code)` call, exactly as the design doc specifies and the `guest_interp.rs` reference prototype already demonstrated. Not a nativecore `Intent` -- there is no "exit" Intent to map to. |

### The fd table

Real Linux `open`/`openat` return small integers the kernel manages;
`Intent::FileOpen`'s real Win32 call (`CreateFileA`) returns a `HANDLE`
(effectively an opaque pointer-sized value), not a small integer.
`src/intent_map.rs`'s `FdTable` bridges this: `openat`/`open` allocates the
next small integer (starting at 3, after the conventional
stdin/stdout/stderr slots this crate does not otherwise model) and records
`fd -> real HANDLE` in a table; `read`/`close` look the real handle up by
that fd. A `read`/`close` for an fd this table never opened (or already
closed) is an honest `UnknownFd` fault.

### Explicitly NOT supported (syscalls) -- and why

- **`write(fd, ..)` for any `fd != 1`.** nativecore has no "write N bytes to
  an arbitrary already-open handle" `Intent` -- `WriteStdout` is hardcoded to
  stdout, `FileWrite` (see below) is a different, incompatible shape. Always
  an honest `UnsupportedWriteFd` rejection, matching `ape-vm`'s own
  `-ENOSYS` posture (print/report, never silently misroute).
- **Write-mode `open`/`openat`** (`O_WRONLY`/`O_RDWR`/`O_CREAT`/`O_TRUNC` set).
  This was the original brief for this crate's verification program (b) --
  corrected mid-build once `Intent::FileOpen`'s real Win32 args
  (`GENERIC_READ | OPEN_EXISTING`) were checked directly against
  `seam.rs`: it is **read-only by construction**, and there is no separate
  "write to an already-open handle" Intent to route a subsequent `write()`
  to either. `Intent::FileWrite` exists in nativecore, but it is an ATOMIC
  create+write+close in a single native call (`args: [path_ptr, buf, len]`)
  -- it cannot be decomposed across three separate guest syscalls
  (`openat`+`write`+`close`) without this interpreter silently buffering
  guest-invisible state and lying about which syscall "did" the write. The
  honest move (this crate's own discipline, and the design doc's) is to
  reject write-mode opens outright (`UnsupportedOpenFlags`), not force a
  mapping that misrepresents what happened. Verification program (b) was
  changed to a real file **read** round trip instead (`openat`(O_RDONLY) ->
  `FileOpen`, `read` -> `FileRead`, `close` -> `FileClose`), which genuinely
  is a clean 1:1 mapping end to end.
- **`mmap` with `MAP_FIXED` / a non-zero address hint, or file-backed
  `mmap`** (fd != -1). `Intent::Alloc` is a single `VirtualAlloc` call that
  always lets Windows choose the address and is always anonymous; neither
  case has a real Win32 call behind it in nativecore today.
- **`fork`/`execve`/`brk`/`arch_prctl`, and every other syscall number not
  listed above.** No honest nativecore `Intent` corresponds to any of these
  (`SpawnWait` is a *fixed* `cmd.exe /c exit 7` spawn-and-wait, not a general
  process-creation primitive `execve`'s semantics could route through
  without lying about what the guest asked for). Left unimplemented on
  purpose -- an `UnimplementedSyscall` fault, the same "don't pretend"
  posture `ape-vm`'s own `-ENOSYS` documents for its unknown syscalls.

## Memory model

Guest address space is one flat `Vec<u8>` (code + rodata + a zeroed stack
region), allocated once per run and never resized afterward, so
`buf.as_ptr()` stays a stable real host pointer for the run's lifetime.
Every value a guest register holds that represents a pointer -- computed via
`lea`, arithmetic, or returned by a translated `mmap` syscall -- is, by
construction, *also* a valid host pointer: there is no separate guest/host
translation step at dereference time (`src/mem.rs`'s header has the full
rationale, including why this is deliberately not a memory-safety sandbox --
Phase 1 proves the mechanism, matching `ape-vm`'s own non-sandboxed model).

This is the exact mechanism the reference prototype `guest_interp.rs` (built
earlier in this project) proved and then hit a real bug in: a RIP-relative
`lea` must add the buffer's real base address (`code.as_ptr() as u64`), not
just the guest-relative displacement, or the resulting "pointer" segfaults
when dereferenced. `src/decode.rs::resolve_addr` and every call site are the
fix; `tests/verify.rs::regression_rip_relative_lea_then_dereference` is the
regression test that reproduces exactly this bug shape (lea a RIP-relative
pointer, then dereference it) and asserts both the computed address and the
dereferenced value are correct.

`call`/`ret` push/pop the return address as a guest-image-*relative offset*
(not a host pointer) -- it is only ever consumed by this interpreter's own
`ret` handling, never handed to a real Win32 call, so there is no reason to
promote it to a host address (and doing so would make the `ret`-side
bounds-check against a corrupt/underflowed stack impossible). Ordinary
`push`/`pop` of a general register is unaffected and still moves the
register's raw 64-bit value verbatim, including real host pointers a
program computed via `lea`.

## How the test programs were built

This box has no `nasm`/`ml64`/`as` available (checked via `where`, all
empty). Every guest program in `examples/*.rs` and `tests/verify.rs` is
hand-encoded machine code -- the same technique `guest_interp.rs` used:
literal opcode bytes with a comment showing the mnemonic each sequence
corresponds to, plus `src/testutil.rs::Emitter` (a tiny byte-pushing/label-
patching helper, not a real assembler) to keep relative-displacement
arithmetic from being done by hand at every call site.

## Real verification

Three real, non-trivial guest programs, each run for real:

- **`examples/verify_loop_cond.rs`** -- a loop with a real `cmp`/`jl`
  conditional branch, printing `AAA\n` via three separate real `write`
  syscalls (proves `cmp`/`jcc`/`add`/loop control flow, and `lea` for the
  message pointers).
- **`examples/verify_file_roundtrip.rs`** -- `openat` + `read` + `close` on a
  real file on disk (path passed as `argv[1]`), echoed back via `write(1,
  ..)`. Proves `FileOpen`/`FileRead`/`FileClose` route correctly through
  `do_intent` to real `CreateFileA`/`ReadFile`/`CloseHandle` calls.
- **`examples/verify_mmap_write.rs`** -- `mmap` (anonymous) -> real
  `VirtualAlloc`, the guest writes 8 real bytes into the returned region via
  two `C7`-form stores, then reads them back through a *separate* real
  `write` syscall (`write(1, mmap_ptr, 8)`), printing `MMAP-OK\n`. Proves the
  mapped memory is genuinely both writable and readable by the host, not
  just that the `Alloc` call itself returned success.

All three are `[[example]]` binaries run with `ExitMode::Real` -- their
`exit` syscall really calls `ExitProcess`, exactly as the design doc
specifies. `tests/verify.rs` spawns each as a real child process (calling
the real `ExitProcess` in-process here would kill the `cargo test` harness
itself) and asserts on real captured stdout and the real process exit code.
`tests/verify.rs` also has the RIP-relative-lea regression test (in-process,
`ExitMode::Return`, no subprocess needed since it never reaches `exit`) and
a handful of fault-path sanity checks (unimplemented opcode, unimplemented
syscall, write to a non-stdout fd, write-mode open) proving each is a clear,
catchable `GuestFault`, never silent wrong behavior.

Run everything: `cargo test -p agenterm-guestcore -- --nocapture` (the
`--nocapture` surfaces each program's real captured stdout/exit status via
`eprintln!` inside the test). Run a program directly:
`cargo run -p agenterm-guestcore --example verify_loop_cond`.

## Scope boundaries (from the dispatch instructions)

Does not touch `crates/agenterm-nativecore/src/**` (only depends on it as a
library), does not touch `crates/agenterm-dynacore/**`, is not wired into
`execute_inner`/the product script path (a later round's job, same
discipline nativecore's own Phase 1 followed), does not attempt aarch64
decoding.
