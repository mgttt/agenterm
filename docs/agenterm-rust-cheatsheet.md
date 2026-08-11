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
- Distinguish GUI-thread-only APIs from worker-safe I/O. Do not block the event
  thread on clipboard reads, PTY waits, filesystem retries, or IPC round trips.
- Retry only documented transient errors, with a strict attempt/deadline bound.

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
- `RegisterClassW` is process-global and parallel windows can race to register the same stable class. Treat `ERROR_CLASS_ALREADY_EXISTS` as success for an application-owned unique class instead of serializing tests or rejecting the second window.
- `ImmAssociateContextEx` is optional on Windows installations without East Asian input support. When the public IME-enable contract is best-effort and has no error channel, a false native result must not terminate an otherwise functional terminal; preserve typed failures only where the public contract can report or safely surface them.
- For incremental Adler-32 SIMD, accumulate byte and weighted sums across the standard bounded reduction chunk and take the modulus once per chunk. Reducing every 16-byte vector block can make a mathematically correct SIMD kernel slower than the scalar implementation.
- Keep checksum correctness tests broad without making them cubic: test every input length one-shot, every split only for representative boundary lengths, and deterministic multi-chunk streams. A test that recomputes every length at every split can process billions of bytes and hide the implementation result behind test design failure.
- Measure ISA paths against a same-source forced-scalar PE. Alternate the two public journeys while both hosts remain live, expose the owned operation duration in the CLI receipt, require byte-identical output, and decide from final PE bytes plus paired p95 rather than process-launch timing.
