# Rust condensed manual

Practical reference for Rust work in this repository. This is not a language
tutorial. It records local contracts and recurring failure modes that general
Rust knowledge does not reveal. Product ownership lives in `PRD.md` and its
module PRDs; source ownership lives in `plan/ARCHITECTURE.md`; agent workflow
lives in `AGENTS.md`. Those files remain authoritative when this manual and a
product decision differ.

Update this document when a Rust lesson is proven by code, target compilation,
tests, emitted assembly, a black-box journey, or a production failure. Do not
record guesses, one-off preferences, or a second living source map.

---

## 0. Before editing

1. Locate the owning PRD and architecture boundary.
2. Identify the exact package, features, targets, public black-box owner, and
   safe failure result.
3. Search every consumer of a changed geometry, protocol, feature, or native
   contract before the first build.
4. Use a task-specific target directory when isolation matters:

```powershell
$env:CARGO_TARGET_DIR = 'target/my-leaf'
cargo clippy -p owning-package --all-targets -- -D warnings
```

Do not share that directory with another active Cargo process. Remove it after
the owning evidence, after resolving and checking that it is the intended
repo-local target.

The repository is pinned by `rust-toolchain.toml`. Do not solve a compiler
failure by silently changing the toolchain, edition, target, linker, or global
Cargo jobs.

---

## 1. Pick the correct layer

| Concern | Owner | Must not own |
|---------|-------|--------------|
| OS-neutral mechanism contract | `crates/agenterm-platform/src/contract/*` or a narrow facade | AgenTerm product names, Fleet, scripts, navigation policy |
| Native mechanism | `crates/agenterm-platform/src/adapters/{windows,unix,linux,macos}/**` selected by `selected.rs` | Product gesture meaning |
| Shared product semantics | `src/frontend/*`, `src/ui_*.rs`, `src/ui_geometry.rs` | `windows_sys`, winit, X11, libc |
| Small host-neutral UI kernels | `crates/agenterm-ui-core` | Window handles, PTYs, product authority |
| Product-specific state | owning binary/module | Raw OS APIs or duplicated native adapters |

`agenterm-con` is intentionally a separate small package, but separation is
not permission to duplicate mechanisms. It may differ from `agenterm` in UI and
authority while reusing PTY, pixel, font, clipboard, filesystem, input, and
failure contracts.

If product code needs `windows_sys`, `libc`, `core::arch`, a raw handle, or an
`unsafe` block, stop and move the mechanism boundary first. Boundary tests are
expected to reject such leakage.

---

## 2. Cargo features are contracts

A build with many enabled features is weak evidence. Cargo unifies features, so
another dependency can accidentally make an undeclared module, OS import, or
optional crate available.

For a new or changed facade, run its isolated graph:

```powershell
cargo check -p agenterm-platform --no-default-features --features filesystem-publish
cargo test  -p agenterm-platform --lib --no-default-features --features filesystem-publish
```

Rules:

- A narrow feature lists every optional dependency and `windows-sys` API module
  needed by its selected adapter.
- A broader feature may depend on a narrow one; do not make the narrow feature
  depend on the full product surface merely to make compilation pass.
- Gate the facade, selected adapter, imports, and tests consistently.
- Dev dependencies can hide production graph mistakes. Inspect and test the
  normal graph separately when size or isolation matters.
- `cargo test FILTER` reporting `0 tests` is not success. List tests or correct
  the feature/filter until the owning tests actually run.

Package boundaries improve cold-build isolation and make feature leakage
visible. They do not by themselves reduce linked size; the linker may already
discard unused code.

---

## 3. FFI and native adapters

Native calls belong behind typed platform contracts. A sound adapter states:

- which thread may call it;
- who owns every handle, pointer, allocation, and callback lifetime;
- which inputs are validated before FFI;
- how absence differs from `Unsupported` and operational `Failed`;
- what cleanup runs on every partial-failure path;
- whether success means visibility, atomic replacement, or durable storage.

Windows checklist:

- Convert paths/text to bounded NUL-terminated UTF-16 at the adapter edge.
- Read `GetLastError` through `last_os_error()` immediately after the failing
  call; another FFI or allocation can overwrite thread-local error state.
- Use RAII for GDI objects, handles, clipboard allocations, capture ownership,
  and process resources. Transfer ownership only after native success.
- A Windows `PROCESS_INFORMATION.hThread` is required through suspended setup
  and `ResumeThread`, but not for ordinary process wait/termination afterward.
  Once resume and PID validation succeed, close that `OwnedHandle` immediately;
  keep only `hProcess` unless a typed runtime contract actually operates on the
  primary thread. Never close it earlier than the armed partial-process owner
  can still terminate a failed suspended launch.
- Distinguish GUI-thread-only APIs from worker-safe I/O. Do not block the event
  thread on clipboard reads, PTY waits, filesystem retries, or IPC round trips.
- Retry only documented transient errors, with a strict attempt/deadline bound.
- A process-global native resource needs one lock and one RAII owner across every
  adapter path. In particular, Windows console attach/detach cannot be split
  between a dependency helper and a platform guard: serialize the whole
  `FreeConsole` / `AttachConsole` / `CONIN$` / `WriteConsoleInputW` transaction,
  and verify exact record counts rather than treating a nonzero call as complete.
- A native dependency replacement earns its complexity only when it removes the
  complete production edge and preserves hidden behavior, not when it merely
  rewrites visible calls. Direct ConPTY must retain cancellable overlapped input,
  output draining during pre-24H2 `ClosePseudoConsole`, DSR fragments, build-gated
  flags, suspended Job assignment, quoting/environment lookup and exact wait
  semantics. Start the child suspended, assign its kill-on-close Job, then resume;
  a failed first child needs a fresh ConPTY because its output pump may reach EOF.

Unix checklist:

- Retry `EINTR` where the syscall contract requires it, not indiscriminately.
- Treat file replacement and durability as separate phases: same-directory
  rename gives name atomicity; syncing the parent owns directory durability.
- Keep fd ownership explicit across fork/exec and close-on-exec boundaries.
- Never substitute a symlink-following path convenience API when the contract
  promises a real entry or protected ancestry.

Use official OS documentation for exact flags and ownership semantics. Record
the stable conclusion in code comments or this manual, not a copied article.

---

## 4. `unsafe` discipline

Rust 2024 requires unsafe operations inside explicit `unsafe {}` blocks even
within an `unsafe fn`. Keep those blocks as small as possible.

Every unsafe mechanism needs:

1. A safe public caller that checks lengths, geometry, alignment assumptions,
   integer overflow, target capability, and lifetime.
2. A local safety explanation tied to those checks.
3. A scalar or safe semantic reference where one exists.
4. Boundary and adversarial tests: zero length, short buffers, tails, overflow,
   clipping, close/cancel, and partial native failure.
5. Target-specific compile evidence for every `cfg` implementation.

Do not use `unsafe` to avoid a borrow-checker design problem, to share mutable
GUI state across threads, or to skip a bounded copy without measurement. A
small allocation is preferable to an unowned pointer; a reusable bounded buffer
is preferable once profiling proves the allocation is hot.

---

## 5. SIMD, intrinsics, and assembly

ISA specialization is justified for compact, stable kernels with a clear byte
or pixel contract. Current good examples are alpha-mask XRGB composition and
XRGB-to-RGB8 packing. VT parsing, JSON, Unicode width, tree state, and other
branch-heavy policy are poor assembly targets.

Required pattern:

```rust
pub fn safe_kernel(input: &[u8], output: &mut [u8]) {
    let length = checked_common_length(input, output);
    unsafe { selected_kernel()(input.as_ptr(), output.as_mut_ptr(), length) }
}
```

- Keep one scalar truth implementation.
- On x86_64, select optional SSSE3/AVX2 with
  `is_x86_feature_detected!` and cache the function pointer with `OnceLock`.
  SSE2 is baseline for x86_64, not for every x86 target.
- On aarch64, NEON is baseline for repository targets, but still compile the
  target-specific implementation.
- Process vector bodies plus exact scalar tails. Test lengths around every lane
  boundary and compare every output bit.
- Keep CPU detection outside inner loops.
- Do not use a similarly named instruction without checking its polynomial or
  semantics. SSE4.2/Arm CRC instructions compute CRC32C, not PNG's IEEE CRC-32.
- Prefer intrinsics first. Inline assembly is reserved for evidence that the
  compiler cannot retain the required instruction sequence or ABI.
- Native FFI is a mechanism boundary, not automatic size evidence. Compare the
  final PE and raw sections against the implementation it replaces. A system
  codec can still add wrapper/control-flow code or cross one file-alignment
  block; keep it when the measured trade buys shared semantics, less protocol
  code, or stronger output, and record the honest delta instead of claiming a
  size win.
- Do not select an assembly/FFI target from one `cargo bloat` top-symbol row.
  ICF, cold blocks, unwind ranges, and adjacent symbol intervals can charge
  unrelated code to a small leaf. Filter the exact symbol, inspect emitted code
  when needed, and compare total `.text` plus final artifact bytes. A measured
  Windows PTY case showed 7.0 KiB in the top list, while the filtered native
  wait leaf was only 105 B; changing its typed boundary moved the 7.0 KiB label
  to process creation and changed neither `.text` nor PE size.
- Audit const generics and iterator adapters whose type records a container's
  shape. A fixed-schema helper taking `[T; N]` can emit its collection path once
  per used `N` even when the operation is cold and identical. In `agenterm-con`,
  replacing the JSON `object<const N>` helper with one owning `Vec` boundary
  collapsed about 2,445 B of measured specializations to 727 B and reduced the
  same-profile PE by 3,072 B. Apply this only where the saved code outweighs
  allocation/runtime cost; it is not a blanket rule against generics.
- For branch-heavy dispatch, share repeated lookup and validation through plain
  non-generic helpers, but keep fallible command work inside its existing local
  `Result` boundary. A helper taking a closure recreates one monomorph per call;
  flattening `?` into a surrounding function that returns `()` changes the
  error-propagation contract. The measured con control refactor used ordinary
  session/cell helpers, retained per-command `map`/`and_then` boundaries, and
  reduced the final PE by 512 B with `.text` down 720 B.
- Protocol enum tags need one encode/decode authority. Plain non-generic
  enum-owned conversion methods can remove parallel match tables without a
  trait or allocation. Measure sections as well as final bytes: con's compact
  mouse tag unification removed 32 B of `.text`, but PE file alignment kept the
  artifact byte count unchanged, so it is a consistency win rather than a PE
  size claim.
- A fixed CLI schema does not require one generic integer parser instance per
  target type. A single non-generic checked ASCII-to-`u64` kernel can feed
  `TryFrom` for unsigned widths while callers retain their exact error text. It
  must preserve details such as one leading `+`, leading zeroes, empty input,
  non-ASCII rejection, and overflow. In con this kernel emitted as 93 B,
  reduced `.text` by 224 B, and crossed one PE alignment block for a 512 B
  artifact reduction. Keep signed parsing on `FromStr` until its separate
  grammar and range semantics have matching evidence.
- Do not change an unwind-enabled native host to `panic = "abort"` merely to
  remove runtime bytes. `agenterm-con` catches panics at WNDPROC, deferred-work,
  and native-thread FFI boundaries; abort changes that containment contract
  rather than optimizing its implementation. First prove no public robustness
  invariant depends on unwind, or retain the exact-profile unwind graph.

Always inspect emitted release code:

```powershell
$env:CARGO_TARGET_DIR = 'target/isa-check'
cargo rustc -p agenterm-ui-core --release -- --emit=asm
rg 'pshufb|vpmullw|packuswb' target/isa-check/release/deps -g '*.s'
```

Writing intrinsics is not evidence that the instruction survived optimization.
Conversely, a visible Rust loop is not evidence that specialization is needed.
`slice::fill`, copies, iterator reductions, and other mature primitives often
already lower to vectorized runtime/compiler code. Inspect and benchmark first.
Keep a shared safe geometry wrapper when it removes duplicated clipping, but do
not maintain ISA forks without a measured gain.

---

## 6. Bounded concurrency and shutdown

Terminal and GUI code must remain bounded under slow consumers and abnormal
children.

- A queue needs a byte/item capacity, explicit backpressure, and a per-GUI-turn
  drain budget.
- Closing must wake blocked producers/consumers and define whether committed
  tail data remains drainable.
- Dropping the product owner must not strand a worker on a full queue.
- Coalesce wakeups and latest-only resize requests; pointer frequency must not
  become PTY resize frequency.
- One tab's malformed output, exited child, failed screenshot, or bad request
  must remain local to that tab/request.
- Do not hold a lock while invoking unknown product callbacks unless the
  contract explicitly owns that serialization and tests shutdown/backpressure.
- Use stable IDs across asynchronous work; reject stale tab/epoch completions.

Prefer a small state machine over booleans that permit impossible combinations.
Selection, mouse capture, process lifecycle, publication, and native resource
transfer all benefit from explicit states.

---

## 7. Rendering and performance evidence

Optimize work removed, not just instructions made clever.

1. Separate input/PTY drain, geometry, raster, chrome, screenshot, and present
   timing where possible.
2. Use public `perf-stats`/snapshot/PNG evidence for `agenterm-con`; use the
   owning public UI journey for the main app.
3. Reduce redundant frames, unchanged rows, allocations, resize calls, and
   lock contention before specializing arithmetic.
4. Keep resize chrome responsive while coalescing PTY/VT geometry at the
   trailing edge.
5. Measure cold build, warm build, binary size, frame latency, and throughput
   separately. Improving one does not prove the others.

Never infer performance from source appearance or binary size. A package split
may improve compilation without changing PE bytes; a typed correctness state
machine may add bytes while removing a data-loss or deadlock path. Report both
cost and benefit truthfully.

Release, release-fast, and debug are different artifacts. Do not compare their
sizes as if profile policy were implementation growth.

---

## 8. Cross-platform evidence

Repository delivery spans `{x86_64,aarch64} x {win,lnx,osx}`. One host build
cannot prove code hidden behind another target's `cfg`.

- Put shared semantics outside target modules.
- Keep selected adapter APIs type-identical across hosts.
- Compile aarch64 when adding NEON or pointer-width-sensitive FFI.
- Compile a native/cross Unix consumer when changing a file imported only by
  Unix frontend modules. If the local host lacks its C compiler/sysroot, report
  the missing evidence and leave it to the owning CI/native cell; do not call a
  Windows-only build cross-platform proof.
- Test endian/channel assumptions through semantic bytes. Repository x86_64 and
  aarch64 hosts are little-endian, but scalar code should avoid accidental
  native-byte-order coupling when simple shifts express the contract.

See `AGENTS.md` for current commands and CI cells; do not duplicate that living
matrix here.

---

## 9. Validation ladder

Use the smallest authoritative evidence first:

1. `rustfmt` on touched Rust files.
2. Package Clippy with `--all-targets -- -D warnings`.
3. Pure scalar/contract tests.
4. ISA parity and target compilation.
5. Owning binary tests.
6. Direct public black-box journey.
7. Release artifact and size.
8. Integrated repository/release gate only at the proper boundary.

GUI tests inherit `AGENTERM_NO_ACTIVATE=1`. Use public `wait-*` commands instead
of fixed sleeps. A test that launches a GUI must own endpoint/workspace
isolation and process cleanup.

Do not rerun a large gate to compensate for not knowing which smaller test owns
the behavior. Add or identify the owner.

---

## 10. Review checklist

- Does the code live in the owning layer?
- Does a narrow feature compile in isolation?
- Are all queues, inputs, outputs, retries, and waits bounded?
- Does shutdown wake blocked work and clean owned resources?
- Are native ownership and thread-affinity rules explicit?
- Is every unsafe precondition enforced by a safe caller?
- Does ISA code have scalar parity, tail tests, target compile, and emitted-code
  evidence?
- Did measurement prove specialization rather than source aesthetics?
- Do Windows, Linux, and macOS adapters expose the same neutral contract or an
  explicit parity gap?
- Does a public black-box test prove user-visible behavior?
- Are generated artifacts outside Git and isolated targets cleaned?
- Were changed docs checked with `scripts/doc-redact-check.sh`?

If a recurring answer was hard to discover, add the proven rule here.

### Audit an API boundary before adding a cache

- Trace the full call path before caching an expensive FFI or parser call. A product facade may already own a bounded cache even when the render caller looks uncached.
- Never layer a second cache around an already cached facade without measured evidence for a distinct lifetime or key domain. It duplicates memory, code, invalidation rules, and statistics while hiding the real owner.
- When cache policy is reusable, move the existing single cache implementation into a host-neutral crate and keep platform rasterization or other native FFI behind the miss path. Preserve capacity, negative caching, fallback behavior, and key semantics during the move.

- `bool::then_some(value)` evaluates `value` eagerly. Do not use it to guard subtraction, indexing, parsing, allocation, FFI, or any other operation that must not happen on the false path; use `if` or lazy `then(|| value)` instead.
- Compare data-structure choices in the final optimized artifact. In this repository, a generic sorted index for tree depths added 2 KiB more release code than the measured `HashMap` implementation even though it looked simpler; source-level intuition is not PE-size evidence.
- On the pinned Rust 1.97 toolchain, `Ord::min` and `Ord::max` are not stable const-trait calls. Do not mark ordinary geometry helpers `const fn` without a real compile-time consumer; remove unnecessary constness instead of duplicating clear operations with manual branches.
- An associated constructor inside impl Type<'_> does not automatically bind an input borrow to the returned type. For borrowed wrappers, write impl<'a> Type<'a> and accept/return &'a ... / Type<'a> explicitly.
- Do not assume a native window or softbuffer preserves pixels across frames or resize unless its typed contract says so. A frame contract must expose retention, generation, and content validity, then require an explicit `None`, `Full`, or bounded partial commit. Raster directly only into valid retained backing; force full after first allocation, resize, DPI change, failed render, or failed present. Keep a product-owned retained fallback and full-copy only for explicitly transient hosts.
- Keep product damage and native present rectangles as separate typed boundaries even when both are half-open pixel rectangles. Product code owns why pixels changed; platform code owns clipping, coordinate conversion, invalidation, and fallback.
- A Windows paint path must pair a successful BeginPaint with EndPaint, treat PAINTSTRUCT.rcPaint as the OS expose authority, and check each API's own failure convention. A negative DIB height is top-down, so StretchDIBits source Y matches client Y; do not apply a bottom-up inversion.
- Do not report partial-present latency from a timer that ends before the native present call. Add platform timing or use ETW before making that claim.
- Prefer an existing typed native FFI mechanism over a new Rust dependency or handwritten assembly when the OS already owns the operation. Then compare LLVM auto-vectorization, `core::arch` intrinsics, and handwritten assembly in that order; retain an ISA-specific path only when final-artifact size or a public hot-path measurement proves a gain on every supported fallback boundary.
- Terminal damage must originate at model mutation sites, not from byte classification or collision-prone row hashes. Keep it allocation-free and conservative, record old and new cursor overlays, escalate unknown callbacks and viewport identity changes to full, and prove no missed rows with exact `Cell` comparison in tests.
- A successful Windows `BeginPaint` is a transaction even when product rendering panics. Use a guard or an inner unwind boundary so `EndPaint` runs exactly once; do not present a partially rendered buffer after a typed render failure, and treat a short positive `StretchDIBits` scanline count as incomplete rather than success.
- Raw Win32 callbacks may reenter synchronously through `SetWindowTextW`, `ShowWindow`, `SetWindowPos`, focus, capture, and related FFI. Native FFI saves dependencies, not Rust aliasing obligations: never reconstruct a second `&mut State` from window userdata while an application callback or framebuffer borrow is live. Keep stable per-HWND userdata, use a shared bounded queue contract but host-specific typed snapshots, consume or copy pointer-backed parameters inside their original callback, and bound both queue capacity and drain work. A thread-local raw `(WPARAM, LPARAM)` backlog is unsafe when multiple HWNDs share a GUI thread and can retain expired pointers.
- Do not wrap every native callback helper in its own `catch_unwind`. Keep one mandatory boundary around each `extern "system"` callback and one around independently drained deferred work, restore phase/lifecycle state deliberately, and convert panic to typed fail-closed state there. Repeated nested catches add x64 unwind metadata and duplicate branches; accept consolidation only when callback panic cannot cross FFI and a same-profile final PE proves the size gain.
- `catch_unwind` is not a delivery invariant when the artifact profile uses `panic = "abort"`; test-profile success can hide that mismatch completely. Any product promising panic containment needs an unwind profile for its complete dependency graph and a test executed under that exact profile. Cargo package overrides cannot change panic strategy, so isolate the product with a named profile and merge its final bytes at staging rather than silently changing sibling products.
- Rust 1.97's `std` exposes `backtrace-trace-only` specifically for `-Zbuild-std`; paired with `panic-unwind` it removes symbolization/demangling code while retaining catch semantics. This is an owned toolchain boundary, not a casual `RUSTFLAGS` tweak: pin `rust-src`, pass an explicit target even for native builds, scope `RUSTC_BOOTSTRAP` to the custom-std subprocess, and qualify/tests under the matching `con-*` profile. The official con custom-std baseline reduced release-fast unwind from 849,920 B to 790,016 B, mostly through `.text`; the shared GDI+ screenshot adapter made it 790,528 B, and direct platform-owned console input made it 791,552 B while closing two real PTY gaps. Replacing the complete Windows `rmux-pty` production edge with a parity-preserving direct ConPTY/Job/pipe adapter then reduced the same artifact to 761,856 B. Exact-profile evidence is 87 unit, 18 black-box and one control test with zero ignores; the 512 KiB budget remains active. Treat each native leaf independently: behavior or ownership can justify growth, but only final-section evidence may call it a size optimization.
- `RegisterClassW` is process-global and parallel windows can race to register the same stable class. Treat `ERROR_CLASS_ALREADY_EXISTS` as success for an application-owned unique class instead of serializing tests or rejecting the second window.
- `ImmAssociateContextEx` is optional on Windows installations without East Asian input support. When the public IME-enable contract is best-effort and has no error channel, a false native result must not terminate an otherwise functional terminal; preserve typed failures only where the public contract can report or safely surface them.
- For incremental Adler-32 SIMD, accumulate byte and weighted sums across the standard bounded reduction chunk and take the modulus once per chunk. Reducing every 16-byte vector block can make a mathematically correct SIMD kernel slower than the scalar implementation.
- Keep checksum correctness tests broad without making them cubic: test every input length one-shot, every split only for representative boundary lengths, and deterministic multi-chunk streams. A test that recomputes every length at every split can process billions of bytes and hide the implementation result behind test design failure.
- Measure ISA paths against a same-source forced-scalar PE. Alternate the two public journeys while both hosts remain live, expose the owned operation duration in the CLI receipt, require byte-identical output, and decide from final PE bytes plus paired p95 rather than process-launch timing.
- Treat `#[inline(always)]` as a measured code-generation exception, not a style preference. Small vector helpers can remain out of line under ordinary `#[inline]`, forcing ABI spills inside a hot loop. Compare emitted assembly and the optimized archive or final artifact, require scalar bit parity and every owning target compile, and document the exact compiler/toolchain evidence. Remove the exception when a compiler upgrade produces the same register-only loop without it.
- A cursor over process-owned `&[String]` should return borrowed `&str`; clone only when a parsed value enters an owned request or state field. Borrow verbs, flags, numeric text, and validation-only tags through the whole parse. This can remove allocator calls, clone error paths, string-drop unwind metadata, and literals together: the con control parser reduced the final PE by 3,072 bytes without changing its grammar or wire protocol. Verify exact errors and round trips because an ownership optimization is still a parser behavior change until tests prove otherwise.
- Centralize repeated fixed-schema formatting at one concrete non-inlined boundary, then compare control-flow spellings in the final artifact. For six con `@TAB_ID` JSON sites, `Option::map_or` saved 384 section bytes but did not cross file alignment; an explicit `match` saved 596 section bytes and 512 final PE bytes. Replacing the remaining `format!` with handwritten stack decimal conversion grew the PE by 512 bytes because constant division, buffer copying, and relocation cost outweighed local fmt scaffolding while integer formatting remained live elsewhere. Keep the measured match, not the assembly-looking version.
- A physical client click on a Win32 top-level window already crosses the OS activation/focus path before product pointer handling. Do not call `SetForegroundWindow`, `SetFocus`, or a facade that reaches them again from that pointer callback merely to focus a product-owned virtual input region. The synchronous focus messages reenter native dispatch and can disturb painting; update product focus state and IME coordinates locally instead. Reserve explicit native focus for startup, keyboard shortcuts that focus without a pointer gesture, and real cross-window activation.
- Repeated one-field JSON results can still monomorphize substantial iterator and collection scaffolding after a general object constructor has been centralized. Route fixed one-field replies through one concrete non-inlined `(name, JsonValue)` boundary and verify exact schemas. Eleven con control/wait sites reduced the staged release-fast PE by 1,536 bytes without changing protocol bytes.
- Replacing every repository call to `is_x86_feature_detected!` does not prove `std_detect` disappears. A dependency or custom `std` path may retain the same cache. Verify the final symbol graph after CPUID/XGETBV replacement; in con, raw detectors matched the standard oracle for SSE2/SSSE3/AVX/AVX2/FMA but `std_detect::detect_and_initialize` remained 1,688 bytes and the final PE grew by 512 bytes. Keep raw detection only when the last linked owner is removed and OSXSAVE plus XCR0 state checks remain exact.

## Assembly and FFI size rule (measured 2026-08-12)

Treat `global_asm!` as a target-specific leaf accelerator, not a default size
optimization. Validate buffers once in Rust, preserve the platform ABI and a
portable fallback, then compare the final staged binary. A tested Win64 GDI
pixel-conversion leaf increased `agenterm-con.exe` by one 512-byte file-alignment
unit and was reverted. Keep assembly only when it removes the original linked
region or measured throughput justifies the retained byte cost. Likewise, an
FFI call saves space only when it makes an entire Rust implementation family
unreachable; `windows-sys` declarations themselves are effectively zero-cost.

For native filesystem FFI, separate caller-owned paths from paths constructed
under a platform invariant. Arbitrary staging paths still need physical-parent,
symlink, identity and destination-type checks. A sibling temporary exclusively
created from one already-canonical parent may skip rediscovering that parent at
publication, provided callback output and the destination are revalidated and
all pre-publication failures still remove the temporary. Keep the OS adapter
mechanism-only: prepared UTF-16 paths, atomic replace, durability and bounded
sharing retries belong there; product path policy does not. Removing three
redundant canonicalization passes from con's atomic screenshot/snapshot path
reduced the staged PE by 3,584 bytes; merely wrapping them in FFI would not.

When a Windows-native field admits a tiny fixed ASCII vocabulary, compare its
`OsStr` as UTF-16 units instead of calling `to_str`, `to_string_lossy`, trimming
and allocating lowercase text. Keep grammar distinctions explicit: a PATHEXT
entry may have one leading dot, while `Path::extension` has already removed it;
reject extra units and unpaired surrogates rather than normalizing them. Sharing
one exact `.exe`/`.com` leaf removed 1,024 bytes from con's staged PE. This rule
does not apply to user text or general Unicode case folding.

Trace constrained native text backward through its producer. Optimizing the
final comparison does not remove `to_string_lossy`, `split`, `format!` or an
intermediate `collect` that still prepares its input. When the complete grammar
is genuinely tiny, parse it once with a bounded native-unit state machine and
emit only typed/canonical outputs. Preserve subtle fallback semantics explicitly:
Windows PATHEXT distinguishes an absent or all-empty list from a nonempty list
whose entries are unsupported. Streaming that complete grammar, plus exact
ASCII-wide environment-key comparison, removed another 2,048 bytes after the
fixed-extension leaf had already landed.

Choose a container from the complete lifecycle, not only asymptotic lookup. A
small environment map built once, overwritten a few times, then consumed in
sorted order before one FFI call does not need a generic tree node engine. A
concrete sorted `Vec` with manual binary insertion preserves ordering and
last-write semantics while making allocation, split and traversal families
unreachable. In the ConPTY environment block this removed every linked BTree
symbol and reduced the staged PE by 7,680 bytes. Do not generalize this to
long-lived or mutation-heavy maps; use measured cardinality, lifecycle and the
final link map.

After specializing a container, trace its producer again. On Windows,
`std::env::vars_os` already reaches `GetEnvironmentStringsW`, but it also
materializes owned key/value objects before product overrides are applied. A
platform adapter may instead own the native block lifetime directly: pair
`GetEnvironmentStringsW` with `FreeEnvironmentStringsW`, bound the terminating
double-NUL scan, recognize hidden `=C:` drive keys by their second `=`, and
stream-merge validated case-insensitive overrides into the Unicode block passed
to `CreateProcessW`. Keep this Windows-only mechanism behind the neutral PTY
contract; Unix environments must preserve their native byte semantics. This
follow-up removed another 1,024 staged bytes, but only after a same-HEAD A/B
comparison separated an unrelated concurrent size change from the experiment.

For a tiny fixed input schema, do not retain a general owned JSON DOM merely
because the same module needs a structured output writer. Keep the boundaries
asymmetric: scan and validate the complete input, store byte spans for known
scalar fields, decode object keys only for semantic comparison or diagnostics,
and skip unknown values without allocating their trees. Duplicate detection
must compare decoded keys, including `\u` spellings, at every object depth.
Preserve input, depth, node, field and decoded-string budgets and reject trailing
data. In con this removed the last configuration DOM owner while preserving the
snapshot/control writer and reduced the staged release-fast PE by 1,536 bytes.

Model filesystem path provenance before choosing normalization. An arbitrary
caller-owned staging path needs physical-parent, link and identity checks. A
temporary exclusively created by the platform beside a destination does not
need to rediscover those relationships, but it still must freeze an absolute
path before callbacks, validate the parent directory and revalidate callback
output. Keep these as separate typed/facade paths rather than a boolean that can
silently weaken the public publisher. On Windows, bounded `GetFullPathNameW`
plus `GetFileAttributesW` removed con's last std filesystem canonicalization
owner and saved one 512-byte PE alignment unit; Unix retained canonical parent
resolution behind the same provenance-specific facade.

For Win32 clipboard writes, the movable global allocation is the final writable
destination, not merely an opaque sink. Count encoded UTF-16 units with checked
arithmetic, allocate once, lock, encode directly, and append the required NUL.
Do not collect a temporary `Vec<u16>` only to memcpy it into `GlobalAlloc`.
Ownership remains the hard boundary: call `GlobalFree` on every failure before
`SetClipboardData`, and never free after that call succeeds. This direct encoding
removed one allocation/copy per selection and one 512-byte PE alignment unit.

Every platform Cargo feature must activate the native declaration features used
by its own adapter. Do not rely on a product's unrelated feature union to make
Win32 functions compile: test the minimal capability graph as well as the real
product graph. `pty` needs `Win32_Security` because windows-sys gates process,
pipe and Job creation declarations through that module even when no product
authorization policy is involved.

## Floating text and clamp linkage (measured 2026-08-12)

Keeping `f64` geometry does not require keeping the standard float text runtime.
A single `FromStr<f64>` owner retains `dec2flt`; `f64::clamp` also retains its
invalid-bound panic plus `Debug`/`flt2dec`, even when product bounds are ordered
constants. For bounded configuration and CLI schemas, parse decimal syntax with
an integer significand and decimal exponent, reject non-finite overflow, then
convert once at the typed boundary. Use an explicit ordered comparison helper
when NaN behavior must match `clamp`; permit `clippy::manual_clamp` only with a
measured link-size reason. Verify removal in the final link map, because source
search alone cannot prove the formatting family became unreachable.

## Parse only the transport a consumer can instantiate

A shared enum may support more mechanisms than a small consumer. Calling its
generic `FromStr` and rejecting an unused variant afterward still links every
parser branch. Prefer a platform-owned typed constructor for the mechanism set
that the caller can actually instantiate, while keeping the generic constructor
for richer consumers. In the con control path, a native named-pipe/Unix-socket
constructor made the entire `core::net::parser` family unreachable and reduced
the staged PE by 6,656 bytes without removing TCP support from the workbench.
This is mechanism-specific linkage, not an authorization profile.

## Windows temporary paths through the platform facade

Do not call `std::env::temp_dir` from a Windows-only adapter merely for a debug
or scratch path. Reuse the platform runtime-directory contract and implement its
Windows leaf with `GetTempPathW`: pass a writable UTF-16 buffer, treat a returned
length at least equal to capacity as a resize request, cap allocation, and keep
a non-panicking fallback. This removed the last con owner of the standard temp
directory routine and saved one 512-byte PE alignment unit while centralizing
the FFI behavior for other products.

## Prefer deterministic sorted storage for small read-heavy maps

`HashMap` can retain random seeding and hashbrown code even when a consumer has
only two map owners. For a bounded cache whose lookups dominate expensive new
value construction, a sorted `Vec<(K,V)>` gives contiguous O(log n) lookup and
acceptably cold O(n) insertion. Recompute the insertion index after FIFO
eviction; an index calculated before removal is stale when a lower key was
deleted. For large static tree batches, sort `(id,index)` once and binary-search
parents to retain O(n log n) behavior and deterministic duplicate diagnostics.
Measure the final link: this pair removed the complete hashbrown/RandomState
family from con and saved 2,048 staged bytes.

## Generic sort can dominate a tiny specialized index

Replacing a hash map with a sorted vector is incomplete size work if
`slice::sort_unstable` becomes the new last owner. Its adaptive generic
monomorphization can be several KiB. When the contract only needs deterministic
O(n log n), a small iterative heapsort provides bounded stack, no auxiliary
allocation, and much less linked code. Preserve total ordering details used by
diagnostics: sorting `(id,input_index)` ensures duplicate IDs still report the
second input occurrence. In ui-core this removed the full generic sort family
and saved 4,096 staged bytes while retaining the 20,000-node deep-tree test.
