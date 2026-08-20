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

Treat generated-code cache identity as one atomic contract. Any transpiler
emission change that bumps `RH_CODEGEN_REVISION` must update the public-contract
and native-pack fixture pins in the same commit; run both owning tests before
the broad AOT pipeline. A stale pin is a delivery failure, not evidence that
the new emitter should reuse an old cache revision.

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

For a host callback table borrowed during construction but invoked later,
separate the two lifetimes explicitly. Copy or own descriptor names before the
constructor returns, retain callback contexts until the native handle is closed,
and destroy the handle before releasing those contexts. Null the output handle
before validating the table so every failure path has the same postcondition.
Treat parameter, result and guest-memory pointers as call-scoped borrows; bound
arity before allocation, require the exact result count, and translate callback
failure into typed fail-closed state without unwinding across C. A Swift wrapper
should own stable pointer storage rather than rely on `Data.withUnsafeBytes`
beyond its closure.

Do not describe elapsed-time checking after a synchronous native callback as a
timeout: it cannot prevent a hang, and latching based on device speed makes a
deterministic guest nondeterministic. Preemption requires a different ownership
model (worker execution, copied/transactional memory and cancellable work), not
a stopwatch around borrowed memory on the owner thread. For trusted app-compiled
callbacks, require each implementation to be finite and nonblocking, then bound
untrusted guest amplification with a per-lifecycle quota charged before dispatch.

`catch_unwind` returning a status is not enough after a mutable runtime operation.
The handle-aware panic branch must mark the instance permanently failed, restore
any lifecycle/phase guard and invalidate cached outputs that could represent
partially mutated state; only inspection and close remain valid. Prove that
transition under the exact unwind-enabled delivery profile, because the ordinary
test profile does not establish that a release artifact can catch at all.

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
- Model native clipboard reads as bounded one-shot work: the platform worker
  owns native retry/blocking mechanics and wakes on success, typed failure,
  panic, and disconnect. The product retains stable target identity and
  revalidates target plus focus before committing text; dropping the receiver
  on tab/window close must never strand or block the worker.
- Treat a human-editable paste review as a second asynchronous identity
  boundary even when its native modal runs a nested GUI message loop. Keep raw
  handles and dialog callbacks in the platform adapter; after confirmation,
  re-normalize and re-bound edited text, then revalidate stable target, focus,
  epoch and terminal mode before the only PTY write. Carry an explicit origin
  bit so CLI/control paste never inherits an interactive modal accidentally.
- Retry only documented transient errors, with a strict attempt/deadline bound.
- Do not use the whole Windows desktop (`HWND = 0`) as deterministic UIA success
  evidence: unrelated providers can recycle, reject calls, or exceed the bounded
  deadline. Let desktop-wide ABI probes typed-fail when a provider is unavailable,
  and own success semantics with a child-owned native window fixture plus the
  public black-box journey. `InvokePattern::Invoke` returning does not guarantee
  the target GUI thread has processed `WM_COMMAND`; wait on the owned observable
  effect with a deadline instead of asserting immediately or sleeping blindly.
  A newly visible native proxy can also briefly return a successful but empty
  RuntimeId SAFEARRAY: bounded re-read is valid, but a synthetic id is not,
  because later node resolution must compare the provider's opaque value. A
  window-scoped tree already has a stronger root identity in its caller-supplied
  HWND: encode and validate that root anchor directly, then use opaque RuntimeIds
  only for descendants that must be rediscovered through the tree walker. If a
  descendant still has no RuntimeId, omit that unaddressable branch; never fail
  the whole snapshot or invent an action target that could resolve to a sibling.
  Cross-platform command tests that pass a synthetic HWND must allow the native
  adapter to reject that window before name lookup; prove exact not-found matcher
  semantics with pure data and prove the integrated path with an owned window.
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

**Never take a `#[repr(C)]` enum by value on the C boundary** (proven at
milestone 53). The C side can pass any `int`, and constructing a Rust enum
from an out-of-range integer is *immediate* UB — it happens at function entry,
before the `match` runs — so a `_ =>` wildcard arm only catches
legal-but-unhandled variants; against garbage input it is false comfort.
Machine-code-identical fix that does not move the ABI: take an integer
(`u32` / `c_int`; a `repr(C)` enum is passed as `int`), keep the C header
declaring the enum, `match` on the integer, and map unknown values explicitly.
Derive the discriminants as `Enum::Variant as u32` constants inside the
function (or a macro), never copy the magic numbers — then a rename/reorder/
revalue of the enum follows through at compile time. When the same numbering
is duplicated across several sources (Rust enum, C header, test-side
constants), gate it with a test that parses every source and compares name
sequence AND values in declaration order; comparing only the name *sets*
misses an insert/swap that shifts every later value.

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
- Format diagnostics through the narrowest type that owns their public range.
  `Duration::as_millis()` returns `u128`; formatting it directly can retain the
  complete `u128` decimal formatter even when the native timeout boundary is
  only `u32` or `u64`. Preserve semantics with an explicit checked or saturating
  conversion and test the extreme value before claiming a size win. In the
  Windows PTY adapter this removed the 1,043-byte linked formatter and moved the
  exact custom-std PE from 533,504 to 531,968 bytes. This rule applies to cold
  diagnostics, not to values whose real contract requires 128-bit precision.
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

That inspection can also disprove the optimistic case. In the staged con PE,
`fill_xrgb_rect` emitted two scalar per-pixel store/branch loops rather than
`rep stosd`, SIMD, or a runtime call. A bounded x86-64 `rep stosd` span leaf and
an AArch64 NEON peer retain the same safe clipping facade; spans below 64 pixels
stay scalar to avoid setup cost. Paired exact-state con PEs remained the same
size while `.text` grew 48 bytes and the ISA PE gained exactly one `F3 AB`
signature. A release-mode 200-frame 1920x1080 A/B measured 102.3 ms versus
210.0 ms (2.05x), with final buffers compared bit-for-bit. This is sufficient
evidence to retain a compact ISA fork; GUI frame timing alone was too noisy.

---

## 6. Bounded concurrency and shutdown

Terminal and GUI code must remain bounded under slow consumers and abnormal
children.

A bounded command buffer does not necessarily bound the native work it
schedules. For duration-bearing media or timer protocols, validate record
count and aggregate scheduled duration before dispatch, in addition to byte
length and each record's local range. TinyArcade tone batches, for example,
bound both event count and total sequential duration so a tiny payload cannot
retain the audio owner indefinitely.

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

For resize profiling, drive the real host through `agenterm-con cli
resize-window --width N --height N`, use a blocking screenshot as the render
fence, then read `perf-stats`. A successful command reply only proves that the
native size request was accepted; PNG IHDR dimensions prove that the backing
surface actually changed. End automated journeys with `close-window` so the GUI
event loop, PTYs, listener, and native handles follow the production shutdown
path instead of being abandoned by a test-process kill.

When extracting retained framebuffer storage, migrate its failure contract as
well as its `Vec`. A host that deliberately omits the terminal from its logical
frame cannot silently ignore retained-layer allocation failure: that would
present stale pixels or a hole. Unix HiDPI therefore maps shared
`RetainedFrameError` into its pixel-window/screenshot result, marks storage
valid only after raster succeeds, and commits its product key afterward. Keep
exact dirty-row masks and invalidation keys product-owned when replacing them
with a conservative shared interval would increase repaint work.

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

- A successful Linux compile does not prove dynamically loaded GUI runtime
  libraries exist in the test image. X11 journeys using winit under Xvfb need
  the `libxkbcommon-x11` runtime (`libxkbcommon-x11-0` on Ubuntu); otherwise
  event-loop creation can abort before the product exposes its control endpoint.
  Preserve child stderr in black-box launch harnesses so a missing `dlopen`
  dependency is reported as the first failure instead of a generic startup
  timeout or signal.
- Unix IPC black-box fixtures must create their runtime parent first and set it
  to mode `0700` before launching the product. A default temp directory created
  under umask `022` is commonly `0755`; the product must reject that endpoint as
  unsafe rather than weakening ownership checks for CI.

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

Keep direct terminal automation distinct from focus-routed UI automation.
`send-keys` deliberately targets a terminal, while `send-ui-keys` must traverse
workspace shortcuts and the current composer/terminal focus owner exactly like
a keyboard event. For composer black-box evidence, query current physical input
bounds through `ui-snapshot`, click those native client coordinates, and prove
both the pre-submit PTY absence and post-submit terminal result.

An IME automation hook must inject the platform-neutral `ImeEvent` after native
decoding, not manufacture Win32 messages or bypass product focus routing. Keep
preedit and commit distinct, bound text and cursor indices at CLI plus wire
decode, expose both composer and active-terminal preedit in structured state,
and report terminal commit success only after the complete PTY write. Such a
hook proves product routing and failure transactions; it does not prove a real
installed IME's IMM32/TSF keyboard behavior.

Capability facts belong to the selected platform adapter. Once an adapter ships
the normalized mechanism, remove product-side OS exceptions that duplicate an
older unsupported state; otherwise the implementation can work while public
discovery still forces callers to degrade. Keep human acceptance as a separate
product-evidence status rather than misreporting the mechanism capability.

Native IME status is event-driven observation, not a render-loop query. Cache
the typed status on open, focus, keyboard/IME transitions and explicit
structured observation; invalidate only the owning chrome when it changes.
Publish stable typed defaults for unknown state, because background/no-activate
windows legitimately lack a thread-local focused input context.

Native IME acceptance must use a real focused desktop window plus physical
virtual-key or scan-code `SendInput`; `KEYEVENTF_UNICODE` bypasses conversion,
and synthetic `WM_IME_*` can manufacture false preedit/commit evidence. A Rust
`char` value is not a Windows virtual-key code: map ASCII letters to uppercase
`VK_A..VK_Z`, keep digits in `VK_0..VK_9`, reject unsupported characters, and
pair every key down with key up. Final CJK text and observed native preedit are
the behavior evidence; status labels and screenshots are supporting evidence.

Feature-gate an adapter facade on both its contract prerequisites and at least
one concrete provider feature. `input + ime` can expose IME facts without
selecting a pixel host; compiling `run_pixel_window` in that graph leaves its
native/portable implementation module absent. Prove narrow capability features
independently instead of relying on a product's larger unified graph.

Process completion and host lifetime are separate states. A terminal waiter
must publish any exit code before its completion flag with release ordering;
the GUI consumes the flag and status with acquire ordering, retains the final
screen, and closes only on an explicit product action. Do not make a one-shot
`-e` child silently redefine remain-on-exit behavior for the whole host.

State waits belong at the owning event loop, not in client polling loops. Keep
them bounded by count and deadline, key them by stable IDs, wake them from the
same transition that updates observable state, and return the completed typed
state (`child_exit_code` included) rather than only a boolean receipt.

A GUI-lifetime control listener must not spawn one detached thread per accepted
connection. Use a fixed worker count, a bounded connection queue, a separate
bounded GUI-request queue, and a short request-read deadline so incomplete
clients cannot create unbounded threads or retain every worker indefinitely.
Queue saturation may return a bounded busy response; shutdown must clear queues
and wake all blocked workers.

Every deferred control reply must have an owning cancellation path. Before a
tab is removed, fail its waits and pending screenshot reply with the stable tab
ID; before window shutdown, fail all remaining replies. Expose pending counts
for black-box sequencing, and keep Drop cancellation as a fallback rather than
the only path, because dropping the sender otherwise collapses a typed close
into a misleading generic timeout.

An enqueue API that may reject work must not take an owned reply sender before
capacity/busy validation succeeds. Borrow `&mut Option<ReplySender>` (or return
the sender with the error), and transfer it only on acceptance. Otherwise the
caller loses the only typed-error path, sender drop wakes the receiver without
a value, and a deterministic busy rejection is observed as a generic timeout.
Cover both sides: acceptance consumes the sender; rejection preserves it for
the caller to answer exactly once.

Closing a bounded PTY output queue only releases a producer blocked on that
queue; it does not by itself release an OS read or process wait. Teardown must
close product backpressure synchronously, then transfer master/child ownership
to a platform-managed background owner that terminates the child tree, closes
the native pseudoconsole and drops both halves. Never call potentially blocking
`ClosePseudoConsole` on a GUI/event thread. On Unix, a detached reader's
duplicated master fd prevents a mere product-master drop from delivering HUP
and can otherwise strand both reader and waiter.

Do not create one teardown thread per closed PTY. Transfer sessions to one
platform reaper with bounded queueing and per-item panic containment; use an
isolated overflow teardown only when that queue cannot accept ownership. This
bounds normal close-storm concurrency without moving native handles back onto
the GUI thread.

Initialize the PTY reaper before acquiring a native PTY. A lazy first close can
discover thread-creation failure only after ownership has moved into teardown;
dropping that failed task may synchronously run the same blocking native close
on the event thread. Startup failure before acquisition is the bounded result.

On the pinned Rust 1.97 toolchain, `OnceLock::get_or_try_init` is still unstable.
For fallible process-wide initialization without feature gates, store a
`Result<T, (io::ErrorKind, String)>` inside `OnceLock` and reconstruct an
`io::Error` for callers.

When bounded producers and control IPC share one native wake event, service the
bounded control queue on every wake before consuming producer work. A producer
that reposts wakes for its remaining backlog can otherwise prevent the event
loop from reaching `about_to_wait` and starve the very command intended to stop
that producer.

For multi-session frontends, make producer work a global per-event budget, not
a full budget independently granted to every session. Divide the fixed byte
budget across live sessions (while preserving progress for each) so tab count
cannot multiply GUI-thread latency.


A bounded request queue does not by itself bound event-loop latency: one GUI
callback can still drain every queued heavy request. Set the callback batch
below the maximum simultaneous worker count, atomically take that fixed batch
and report whether backlog remains, then repost the native wake when needed.
Coalesce producer wakes on the empty-to-nonempty transition. Keep wait/deadline
evaluation outside that dispatch budget so timeout progress is never deferred
by load.

Latest-only command coalescing must preserve both queue order and reply
ownership. Concurrent IPC workers may not have enqueued a logical burst when
the GUI drains its first bounded batch, so inspecting only the current queue
tail is timing-sensitive. Use a short fixed deadline measured from the first
supersedable request, make every other command an ordering barrier, submit the
last value once, and complete every absorbed reply. Never reset the deadline on
later arrivals or leave those replies owned only by the normal pending-wait
registry; window teardown must fail them explicitly.

Deferred frame operations need one global owner when they change active-session
state. A per-tab pending screenshot slot is insufficient: draining several
requests can switch active repeatedly and render only the last target, stranding
the others. Cover pending-render and background-encode as one bounded state,
reject overlap explicitly, and move PNG encoding/publication off the GUI thread
while retaining a shared one-shot reply slot for immediate close cancellation.
Keep the pending operation at frontend scope, not inside the target session: a
later tab selection must not make progress depend on that target remaining
active. At render entry, capture the latest desired active tab, render the
target, copy the frame for background work, then restore and invalidate the
visible owner.

Do not use an omitted frame commit to suppress presentation: the compatibility
contract treats an unspecified write as a full write. Capture-only rendering
needs an explicit discard receipt that is accepted for retained and transient
backings, invalidates backing content, and makes every host skip native present.

Feature-isolate diagnostics too. An optional adapter trace must not call an
unrelated feature-gated module merely to choose a log directory; use the native
temporary-directory contract when that is the documented destination. Full
product feature union can otherwise hide an undeclared dependency indefinitely.

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
- Cache a derived topology beside its authoritative owner only when mutation is closed over a small API. Keep the shared typed algorithm as the validation/rebuild authority, update append-only mutations in O(1), rebuild after relationship-changing mutations, and expose immutable slices to repaint code. Parallel vectors require tests that preserve equal length and ordering; a render-local cache or second topology algorithm creates competing authority.
- Keep independently consumable ISA kernels behind independent lazy selectors. A struct that stores function pointers for unrelated blend and pack operations can retain an unused assembly kernel merely because initialization takes its address. Confirm DCE with the final consumer binary; a zero-byte unused kernel is useful evidence even when the PE file stays in the same alignment bucket.
- On the pinned Rust 1.97 toolchain, `Ord::min` and `Ord::max` are not stable const-trait calls. Do not mark ordinary geometry helpers `const fn` without a real compile-time consumer; remove unnecessary constness instead of duplicating clear operations with manual branches.
- An associated constructor inside impl Type<'_> does not automatically bind an input borrow to the returned type. For borrowed wrappers, write impl<'a> Type<'a> and accept/return &'a ... / Type<'a> explicitly.
- Do not assume a native window or softbuffer preserves pixels across frames or resize unless its typed contract says so. A frame contract must expose retention, generation, and content validity, then require an explicit `None`, `Full`, or bounded partial commit. Raster directly only into valid retained backing; force full after first allocation, resize, DPI change, failed render, or failed present. Keep a product-owned retained fallback and full-copy only for explicitly transient hosts.
- A trailing-edge PTY resize debounce does not by itself optimize live window resizing. If every native size notification reallocates or invalidates backing and forces a full raster, the expensive work still occurs at pointer frequency. Encapsulate host interaction phases such as Win32 `WM_ENTERSIZEMOVE` / `WM_EXITSIZEMOVE` in the platform contract, keep Unix/macOS debounce as the fallback, and reuse or scale validated retained pixels during the interaction only when the backing contract can prove their lifetime. Perform one exact grid/PTY convergence and full raster at the end. Do not leak HWND messages into product UI or claim an event enum alone as an optimization.
- Keep product damage and native present rectangles as separate typed boundaries even when both are half-open pixel rectangles. Product code owns why pixels changed; platform code owns clipping, coordinate conversion, invalidation, and fallback.
- A Windows paint path must pair a successful BeginPaint with EndPaint, treat PAINTSTRUCT.rcPaint as the OS expose authority, and check each API's own failure convention. A negative DIB height is top-down, so StretchDIBits source Y matches client Y; do not apply a bottom-up inversion.
- Do not register a retained pixel window with Win32 `CS_HREDRAW` or `CS_VREDRAW`. Those class styles ask User32 to invalidate the complete client on every width or height change and override product damage tracking. Let exposed regions, typed `InvalidateRect` requests, and the settled geometry redraw own paint. A 16-step con resize journey fell from 35 to 18 frames, 17 to 8 full candidates, and 25.715 ms to 16.302 ms of measured native present time after removing the flags, with identical PNG geometry and zero present failure/copy.
- Win32 live resize has an explicit `WM_ENTERSIZEMOVE` / `WM_EXITSIZEMOVE` lifetime. Snapshot both messages at the normal reentrancy boundary and expose host-neutral begin/end events. While active, update current client metrics but do not advance a valid retained frame generation or rerun product raster for each `WM_SIZE`; let GDI scale the last successfully committed top-down DIB under the `BeginPaint` clip. On exit, publish final metrics, advance generation, notify geometry, and explicitly redraw once. Require exact old width/height/pixel-count agreement before reuse and fall back to normal raster otherwise. A 16-step con journey reduced product raster from 20 frames/9 full candidates to 3-5 frames/1-2 full candidates, while 11-13 native presents all succeeded. Linux/macOS continue through ordinary geometry plus trailing-edge debounce until their hosts expose an equivalent phase.
- Do not report partial-present latency from a timer that ends before the native present call. Add platform timing or use ETW before making that claim.
- Prefer an existing typed native FFI mechanism over a new Rust dependency or handwritten assembly when the OS already owns the operation. Then compare LLVM auto-vectorization, `core::arch` intrinsics, and handwritten assembly in that order; retain an ISA-specific path only when final-artifact size or a public hot-path measurement proves a gain on every supported fallback boundary.
- Whole-file configuration reads must enforce their parser limit before an unbounded allocation. Use a narrow platform facade rather than the full filesystem capability: Windows can combine `CreateFileW`, `GetFileSizeEx`, and partial `ReadFile` loops under one owned handle, while Unix can use `take(max + 1)`. Check both the opened-file size and bytes actually read because files can grow; preserve existing link-following and sharing behavior unless the product contract explicitly changes it.
- Windows parent-console output has two native contracts: a real console requires UTF-16 `WriteConsoleW`, while an inherited pipe or file requires UTF-8 `WriteFile`. Detect the former with `GetConsoleMode`, loop on partial writes and reject zero progress. Borrowed `GetStdHandle` values must never be closed; a fallback `CONOUT$` handle from `CreateFileW` must be owned and closed exactly once. Do not add `FlushFileBuffers` to emulate `Write::flush`: pipes, console handles, and durable files have different flush semantics.
- Terminal damage must originate at model mutation sites, not from byte classification or collision-prone row hashes. Keep it allocation-free and conservative, record old and new cursor overlays, escalate unknown callbacks and viewport identity changes to full, and prove no missed rows with exact `Cell` comparison in tests.
- A successful Windows `BeginPaint` is a transaction even when product rendering panics. Use a guard or an inner unwind boundary so `EndPaint` runs exactly once; do not present a partially rendered buffer after a typed render failure, and treat a short positive `StretchDIBits` scanline count as incomplete rather than success.
- Raw Win32 callbacks may reenter synchronously through `SetWindowTextW`, `ShowWindow`, `SetWindowPos`, focus, capture, and related FFI. Native FFI saves dependencies, not Rust aliasing obligations: never reconstruct a second `&mut State` from window userdata while an application callback or framebuffer borrow is live. Keep stable per-HWND userdata, use a shared bounded queue contract but host-specific typed snapshots, consume or copy pointer-backed parameters inside their original callback, and bound both queue capacity and drain work. A thread-local raw `(WPARAM, LPARAM)` backlog is unsafe when multiple HWNDs share a GUI thread and can retain expired pointers.
- Do not wrap every native callback helper in its own `catch_unwind`. Keep one mandatory boundary around each `extern "system"` callback and one around independently drained deferred work, restore phase/lifecycle state deliberately, and convert panic to typed fail-closed state there. Repeated nested catches add x64 unwind metadata and duplicate branches; accept consolidation only when callback panic cannot cross FFI and a same-profile final PE proves the size gain.
- `catch_unwind` is not a delivery invariant when the artifact profile uses `panic = "abort"`; test-profile success can hide that mismatch completely. Any product promising panic containment needs an unwind profile for its complete dependency graph and a test executed under that exact profile. Cargo package overrides cannot change panic strategy, so isolate the product with a named profile and merge its final bytes at staging rather than silently changing sibling products.
- Rust 1.97's `std` exposes `backtrace-trace-only` specifically for `-Zbuild-std`; paired with `panic-unwind` it removes symbolization/demangling code while retaining catch semantics. This is an owned toolchain boundary, not a casual `RUSTFLAGS` tweak: pin `rust-src`, pass an explicit target even for native builds, scope `RUSTC_BOOTSTRAP` to the custom-std subprocess, and qualify/tests under the matching `con-*` profile. The official con custom-std baseline reduced release-fast unwind from 849,920 B to 790,016 B, mostly through `.text`; the shared GDI+ screenshot adapter made it 790,528 B, and direct platform-owned console input made it 791,552 B while closing two real PTY gaps. Replacing the complete Windows `rmux-pty` production edge with a parity-preserving direct ConPTY/Job/pipe adapter then reduced the same artifact to 761,856 B. Exact-profile evidence is 87 unit, 18 black-box and one control test with zero ignores; the 512 KiB budget remains active. Treat each native leaf independently: behavior or ownership can justify growth, but only final-section evidence may call it a size optimization.
- Keep `compiler_builtins` in an explicit Rust 1.97 `-Z build-std` root list.
  `core` now contains `f16` formatting paths that reference
  `__truncsfhf2`/`__extendhfsf2`; a target directory carrying builtins without
  the matching reliable-f16 cfg can compile `core` and fail only at final MSVC
  link. The con root list is therefore
  `std,panic_unwind,compiler_builtins`. Prove toolchain changes with an isolated
  cold target: adding the root to a warm command need not invalidate an already
  mismatched archive. A cold Windows x64 build completed in 93.8 seconds and
  restored the 531,968-byte custom-std artifact.
- `RegisterClassW` is process-global and parallel windows can race to register the same stable class. Treat `ERROR_CLASS_ALREADY_EXISTS` as success for an application-owned unique class instead of serializing tests or rejecting the second window.
- `ImmAssociateContextEx` is optional on Windows installations without East Asian input support. When the public IME-enable contract is best-effort and has no error channel, a false native result must not terminate an otherwise functional terminal; preserve typed failures only where the public contract can report or safely surface them.
- For incremental Adler-32 SIMD, accumulate byte and weighted sums across the standard bounded reduction chunk and take the modulus once per chunk. Reducing every 16-byte vector block can make a mathematically correct SIMD kernel slower than the scalar implementation.
- Keep checksum correctness tests broad without making them cubic: test every input length one-shot, every split only for representative boundary lengths, and deterministic multi-chunk streams. A test that recomputes every length at every split can process billions of bytes and hide the implementation result behind test design failure.
- Measure ISA paths against a same-source forced-scalar PE. Alternate the two public journeys while both hosts remain live, expose the owned operation duration in the CLI receipt, require byte-identical output, and decide from final PE bytes plus paired p95 rather than process-launch timing.
- Treat `#[inline(always)]` as a measured code-generation exception, not a style preference. Small vector helpers can remain out of line under ordinary `#[inline]`, forcing ABI spills inside a hot loop. Compare emitted assembly and the optimized archive or final artifact, require scalar bit parity and every owning target compile, and document the exact compiler/toolchain evidence. Remove the exception when a compiler upgrade produces the same register-only loop without it.
- A cursor over process-owned `&[String]` should return borrowed `&str`; clone only when a parsed value enters an owned request or state field. Borrow verbs, flags, numeric text, and validation-only tags through the whole parse. This can remove allocator calls, clone error paths, string-drop unwind metadata, and literals together: the con control parser reduced the final PE by 3,072 bytes without changing its grammar or wire protocol. Verify exact errors and round trips because an ownership optimization is still a parser behavior change until tests prove otherwise.
- Centralize repeated fixed-schema formatting at one concrete non-inlined boundary, then compare control-flow spellings in the final artifact. For six con `@TAB_ID` JSON sites, `Option::map_or` saved 384 section bytes but did not cross file alignment; an explicit `match` saved 596 section bytes and 512 final PE bytes. Replacing the remaining `format!` with handwritten stack decimal conversion grew the PE by 512 bytes because constant division, buffer copying, and relocation cost outweighed local fmt scaffolding while integer formatting remained live elsewhere. Keep the measured match, not the assembly-looking version.
- A physical client click on a Win32 top-level window already crosses the OS activation/focus path before product pointer handling. Do not call `SetForegroundWindow`, `SetFocus`, or a facade that reaches them again from that pointer callback merely to focus a product-owned virtual input region. The synchronous focus messages reenter native dispatch and can disturb painting; update product focus state and IME coordinates locally instead. Reserve explicit native focus for startup, keyboard shortcuts that focus without a pointer gesture, and real cross-window activation.
- Treat native pointer capture plus PTY reporting as one fallible transaction. If capture is acquired before a press report, release it when the write fails; commit `last_reported_cell`, drag ownership, and active-button state only after the report succeeds. Keep physical input best-effort across concurrent child exit, but let control/automation callers receive the write failure and distinguish an application-consumed event from a coalesced same-cell motion that wrote no bytes. When automation splits press/release across requests in a multi-session window, keep one window-scoped typed owner rather than independent session booleans; reject overlap and cross-owner release, and cancel both the owner and active physical gesture before tab activation, creation, close, or shutdown.
- When physical input and public automation share an encoder, make the shared core return the actual fallible delivery result. The physical event path may be a thin best-effort wrapper across concurrent child exit; CLI/control callers must invoke the checked core and propagate write failure. Commit live-view scroll, last-delivered coordinates, and similar delivery-dependent state only after the write succeeds. A unit-returning shared helper silently turns automation receipts into false success.
- Treat editable UI submission to an external sink as an ownership transaction. Move the bounded draft into one submission value without cloning, append transport framing only there, and clear/scroll/mark-delivered state only after the complete write succeeds. On failure remove framing, restore the exact draft, retain retry focus, and expose a bounded typed error; every keyboard, IME, accessibility, and programmatic edit path must clear stale failure state consistently.
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

When a final PE imports `ceilf`, `round`, `roundf` or `truncf`, audit every
linked owner before replacing individual calls. Geometry conversion can use one
shared IEEE-754 bit-level leaf: classify exponent bits, mask fractional bits,
apply the half-unit in significand space, and preserve sign, signed zero,
infinity and NaN payloads. Keep concrete functions non-inlined when many call
sites share them, and compare their result bits against the standard library
over boundary and sampled representations. Con removed all four CRT imports and
one 512-byte PE alignment unit this way. Prefer this portable scalar truth over
assembly until emitted-code or hot-path evidence justifies SSE/NEON dispatch.

On Windows MSVC, replacing `mainCRTStartup` is safe only if the replacement
still reaches rustc's generated C `main`; calling the product function directly
skips `lang_start`, runtime initialization, panic containment and cleanup.
Windows std ignores C `argc`/`argv` and parses `GetCommandLineW`, so `0/null` is
valid for the generated wrapper. Explicitly walk `.CRT$XI*` then `.CRT$XC*`
before it, and `.CRT$XP*` then `.CRT$XT*` after it. Never walk `.CRT$XL*`: the PE
TLS Directory makes those callbacks loader-owned, and manual invocation would
double-run thread cleanup. Test the boundary with a test-only `.CRT$XCU`
constructor that must fire before Rust test main.

If Rust rejects a `#[link_name = "main"]` declaration as a duplicate generated
entry, use the smallest architecture seam rather than reimplementing runtime:
an x86_64 `jmp main` or ARM64 `b main` trampoline preserves the C ABI and return
address. Keep initialization in Rust. Link `vcruntime`/`ucrt` import libraries
explicitly because the removed CRT startup object formerly pulled them in
implicitly. Suppress LNK4210 only after XI/XC/XP/XT and loader-owned XL are all
accounted for. In con this removed 5,120 staged bytes and four startup-only UCRT
DLL families while retaining unwind.

Treat process argv parsing as a platform contract, not an automatic
`std::env::args` choice. For a Windows GUI that accepts native Shell parsing,
`GetCommandLineW` plus `CommandLineToArgvW` can make Rust's generic `OsString`
parser family unreachable. Own the returned pointer with a guard that calls
`LocalFree` exactly once, bound argc and every UTF-16 NUL scan, and return
`InvalidData` for null pointers or unpaired surrogates instead of panicking in
startup. Keep Linux/macOS behind the same UTF-8 `Result` facade.

This is not semantics-free: `CommandLineToArgvW` differs from modern MSVC rules
for ambiguous hand-crafted quote sequences, and loading Shell32 can hurt a
small console process. Require standard-launcher round trips and public CLI
tests before adopting it. In con, which is already a GUI, target-specific cold
A/B reduced the official release-fast PE from 543,232 to 541,184 bytes while
adding `shell32.dll`; that final-link result, not the FFI declaration itself,
justified retention.

When a product uses `-Z build-std` with an explicit target, `cargo clean -p`
without `--target` does not clean that product graph. For con size A/B, clean
both owning packages with the exact target triple before each side. An earlier
484,352-byte incremental argv artifact failed this provenance test; same-HEAD
cold builds established the real 2,048-byte reduction. Never promote a warm or
stale staged byte count into PRD evidence.

For a Windows process that already needs the complete inherited environment
for `CreateProcessW`, share one RAII `GetEnvironmentStringsW` block instead of
adding independent std and `GetEnvironmentVariableW` paths. Keep the block
borrowed, cap the double-NUL scan, and pair it with `FreeEnvironmentStringsW`
exactly once. If product callers need only fixed protocol keys, state an ASCII
key contract rather than silently weakening a general Unicode API.

The con x86_64 scanner is a valid inline-assembly exception because an isolated
cold final PE fell from 540,672 to 540,160 bytes and Windows aarch64 retained a
tested Rust fallback. Inline-assembly scratch registers that are written before
all inputs are consumed must use `out(reg)`, not `lateout(reg)`: LLVM may alias a
`lateout` with an input, and this scanner's first version corrupted a live
pointer and access-violated. Test the raw bounded leaf with synthetic empty,
missing, hidden-drive and truncated blocks, not only the OS-owned happy path.
During parallel `-Z build-std` work, use an exclusive target directory; a
shared target can mix custom `core` and `compiler_builtins` artifacts even when
the source tree is correct.

Do not replace a bounded `String::from_utf16` path with
`WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS)` merely because the latter
is native. Con's Windows argv experiment preserved valid arguments and rejected
an unpaired surrogate, but the isolated custom-std release-fast PE grew from
540,160 to 540,672 bytes. The required-size call, output allocation, second FFI
call and surrounding branches did not remove enough linked Rust machinery to
cross the final artifact boundary. The experiment was reverted. Prefer the
existing Rust conversion unless a caller can lend fixed output storage or a
future link graph proves that the complete conversion family becomes dead.

Likewise, do not hand-assemble a cold glyph-format loop before accounting for
its cache and surrounding control flow. Con first moved GDI gray8 stride and
length validation outside the per-pixel loop; that clear scalar helper grew the
custom-std PE from 540,160 to 540,672 bytes. Replacing its inner loop with a
bounded x86_64 inline-assembly row scanner grew it again to 541,184 bytes. Both
versions passed padding, saturation, short-write and real ASCII/CJK GDI tests,
but glyph conversion runs only on cache misses and no public latency evidence
justified the size cost, so both were reverted. The higher-value native font
target is reusable `HDC`/`HFONT` lifetime, not six arithmetic instructions.

For `CreateCompatibleDC(NULL)`, Microsoft assigns HDC ownership to the creating
thread and invalidates it when that thread exits. Do not make a cached GDI face
`Send` merely to fit a process-global mutex. Con keeps a thread-local RAII set
instead: one active pixel size, lazy family creation, exactly-once
`SelectObject` restoration plus `DeleteObject`/`DeleteDC`, and a complete reset
when size changes. `try_with` and `try_borrow_mut` convert teardown or reentry
into a typed raster failure rather than a panic. A deterministic test rasterizes
94 distinct printable ASCII glyphs and observes one native face creation instead
of 94. The custom-std PE cost is 2,048 bytes (540,160 to 542,208); this is an
accepted native-lifecycle/first-render trade, not a size optimization.

Exact UTF-8 substring search can be a good assembly leaf when the product
contract is narrower than Rust's generic pattern framework. Con `wait-text`
matches each visible physical terminal row independently: it does not join
lines, scan hidden scrollback, normalize Unicode, or fold case, and an empty
needle succeeds. Its x86_64 bounded byte-search assembly preserves exactly that
contract; Windows aarch64 and Unix use a scalar byte oracle. Synthetic byte
matrices plus CJK/emoji cases must agree with slice-window search. Replacing the
wait-text production `str::contains` call reduced the custom-std PE from 542,208
to 537,600 bytes (-4,608), a final-link gain much larger than the leaf itself. A
later symbol build still found the generic pattern/searcher family through
unrelated fixed-character checks, so do not describe this delta as complete
family removal without post-change attribution.

Do not mechanically rewrite every fixed-character `str::contains` as byte-slice
membership to chase that family. In con, changing the remaining IPC and process
convention checks for `':'`, `'='`, and NUL grew the custom-std PE from 537,600
to 539,136 bytes while strip/split patterns still retained the framework. The
experiment was reverted. Final-link A/B and post-change symbols both matter.

The same rule applies when `std` is already a thin native wrapper. Replacing
the three Windows PTY uses of `std::env::current_dir` with a bounded
`GetCurrentDirectoryW` retry loop preserved explicit-path, relative-PATH and
resolved-image behavior, but grew the exact custom-std PE from 537,600 to
538,624 bytes in the initial unpaired observation. A same-state reverse build
established the current baseline as 538,112 bytes, so the attributable cost is
one 512-byte alignment step. The replacement duplicated buffer sizing, directory-change race
handling and error conversion without removing enough standard-library code,
so it was reverted. Keep `std::env::current_dir` until a shared platform
contract, fixed caller storage, or final-link evidence changes that trade.

Do not force tiny leaf helpers across a crate boundary merely to remove source
duplication. The workbench and con each had a six-line UTF-8 prefix clamp for
capture output. Moving it into `agenterm-ui-core` preserved CJK/emoji behavior
but grew con's exact custom-std PE from 538,112 to 538,624 bytes, one 512-byte
alignment step, because the
former product-local loop no longer optimized in place. The move was reverted.
Share protocol types, state machines and substantial kernels; tolerate a tiny
semantic duplicate when the public contract is stable, tests agree and the
final-link cost of a cross-crate call is larger than the maintenance benefit.

A cross-product extraction is justified when it removes an observable semantic
fork rather than only repeated syntax. The workbench and `agenterm-con` once
carried separate VT word, visible/logical-row and clipboard-text rules; con's
triple-click crossed soft wraps while the product contract required one visible
row. `agenterm-ui-core::terminal_selection` now owns the physical-screen kernel,
behind an optional `terminal-selection` feature so unrelated UI-core consumers
do not acquire `vt100`. Product gesture/capture/authority state stays outside.
Require shared-kernel tests plus both product suites, and make no size claim
until an exact-profile final-link A/B is measured.

Before replacing `is_x86_feature_detected!`, prove that the generic detector's
last production owner is in scope. UI-core briefly combined its AVX2 and SSSE3
dispatch into one cached CPUID probe, with OSXSAVE/AVX checks and a bounded
`xgetbv` assembly leaf for XMM/YMM state. The probe matched the standard oracle
and all pixel/con tests passed, but another link owner retained
`std_detect::detect_features`; bloat `.text` grew from 348.5 to 349.0 KiB while
the exact 538,112-byte PE merely hid the increase inside existing alignment.
The probe was reverted. A native ISA probe is a win only after link attribution
shows it removes the complete generic detector or provides measured hot-path
latency that justifies two detectors.

That prerequisite can change after dependency-feature work. VTE's `std`
feature only enables `memchr/std`; its parser API is unchanged without it. Con's
vendored vt100 now disables that feature, so x86_64 ESC scans retain baseline
SSE2 while dropping memchr's AVX2 runtime dispatcher. UI-core then became the
last production `std_detect` owner. One `OnceLock<X86Kernels>` now caches direct
blend and RGB-pack function pointers selected by a narrow CPUID probe; AVX2 is
accepted only with CPUID AVX + OSXSAVE, XCR0 XMM/YMM state from bounded `xgetbv`
inline assembly, and CPUID.7 AVX2. The probe matches the standard oracle in
tests, while the oracle is test-only. Paired custom-std builds measured 538,112
to 537,600 bytes for VTE no-std, then 537,600 to 536,064 bytes after replacing
the final detector. Bloat `.text` fell from 348.5 to 346.5 KiB and no linked
`std_detect::detect_features` symbol remained. This is the required sequence:
remove every owner first, then replace the final authority once.

Model fixed-schema JSON keys as borrowed static data, not owned user data. Con's
bounded JSON parser extracts typed configuration values and never constructs a
generic object tree; every output object key is a compile-time schema literal.
Changing `JsonValue::Object` from `Vec<(String, JsonValue)>` to
`Vec<(&'static str, JsonValue)>` therefore removes one allocation and copy per
field without weakening input bounds or allowing borrowed request data to
escape. Dynamic titles, text and paths remain owned values. The exact custom-std
PE fell from 536,064 to 534,528 bytes, `.text` from 346.5 to 345.5 KiB, and the
public GUI/control suites preserved JSON interoperability. Do not generalize
this representation to a parser that must retain arbitrary input keys.

Apply the same provenance rule to fixed-schema integer values. Con's production
JSON outputs only signed or unsigned integers; decimal fractions belong to the
typed configuration parser and never enter the output tree. `JsonValue` now
stores `u64`/`i64` until final serialization, where a directly declared `itoa`
dependency writes into the existing response buffer. A raw decimal-string
variant exists only under `cfg(test)` for codec interoperability. This removes
one `to_string` allocation per perf, snapshot, dimension and delivery-count
field and reduced the exact PE from 534,528 to 532,480 bytes. Declare a crate
you call directly even when another dependency already happens to pull it in;
transitive availability is not an API contract.

Keep protocol-formatted identifiers typed until the final writer when their
wire spelling is fixed. Con's stable tab ID is a `u64` internally and a JSON
string `"@N"` publicly. A dedicated output-tree variant writes the quote,
prefix and integer directly through the existing `itoa` buffer; it does not
change CLI parsing, workspace identity, window-title formatting or nullable
parents. This removed `format!` allocation from control replies and reduced the
exact PE from 532,480 to 531,456 bytes. Do not reuse the variant for arbitrary
prefixed text: its value is that the type proves the complete wire grammar.

Avoid building a temporary `String` when a fixed-cell painter can consume
borrowed segments under one metrics/clip pass. Con's chrome now paints tree
`@N  title`, composer destination, committed text, IME preedit and cursor from
stack `itoa` digits plus borrowed slices. A pixel oracle compares joined and
segmented CJK text under a non-cell-aligned clip and requires exact framebuffer
equality. This removes three heap constructions from every relevant chrome
repaint while keeping the exact PE at 531,456 bytes. Keep the segmented painter
product-local: the strings are con UI policy, while font rasterization remains
the shared platform mechanism.

For the Windows roaming configuration root, prefer
`SHGetFolderPathW(CSIDL_APPDATA)` with a caller-owned `MAX_PATH` UTF-16 buffer
when that legacy length contract is acceptable. `SHGetKnownFolderPath` returns
COM task-allocated memory and requires `CoTaskMemFree`; do not add that allocator
edge for a path that already fits the product contract. Keep the filename and
schema in the product, expose only the host configuration-root mechanism from
platform, and retain Unix behavior behind the same facade. In con this removed
one 512-byte PE alignment unit after target-specific cold measurement.

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

## Cross-package executable probes

An integration test compiled by cross-target `--all-targets` cannot assume Cargo
provides `CARGO_BIN_EXE_*` for a binary owned by another package. Use
`option_env!` to retain the native running-binary probe while allowing
cross-compilation to keep checking the static contract. Never skip the static
schema, ownership, or evidence checks merely because that target cell cannot
execute the sibling binary.
## Windows supplementary glyphs without a second font stack

`GetGlyphIndicesW` maps UTF-16 code units, so passing a surrogate pair does not
prove one Unicode scalar was mapped. For a selected GDI TrueType/OpenType face,
the small product-neutral path is: read a strictly bounded `cmap` table with
`GetFontData`, parse the big-endian format-12 UCS-4 groups with checked offsets,
then pass the resulting glyph index to `GetGlyphOutlineW` with
`GGO_GLYPH_INDEX`. Keep BMP characters on `GetGlyphIndicesW`, cache at most one
bounded table per live face, and treat absent/malformed tables as local missing
coverage. This handles nominal supplementary outline glyphs; it does not claim
color emoji, variation sequences, or run shaping, which require DirectWrite.

## Integration-test paths are gate ownership

Cargo automatically discovers every `tests/*.rs` file for the package rooted
at that manifest. Pointing a second package's explicit `[[test]]` entry at the
same root-level file does not transfer ownership: both packages run it, under
different profiles and binary-resolution contexts. Product-specific GUI and
black-box tests must physically live under the owning package's `tests/`
directory, use package-relative `[[test]]` paths when explicit registration is
needed, and publish repo-relative evidence paths separately. Confirm ownership
through Cargo metadata, not only by observing one green invocation.

The same ownership rule applies to repository source audits. A scanner rooted
at the workbench `src/` tree does not follow a package's `[[bin]]` target after
that source moves under `crates/<package>/src`. When physically separating a
package, update every boundary, native-API, hygiene, and architecture scanner
to include the new source root explicitly; otherwise a correct Cargo move can
silently create an audit blind spot.

## Shared integration-test helpers are per-crate dead code

Every `tests/*.rs` compiles as its own crate, so helpers moved into
`tests/common/mod.rs` (e.g. a shared C-toolchain discovery module) look dead
to every test crate that does not reference them — gate the module with
`#[allow(dead_code)]` exactly like the existing `system_libs` module, and say
in the comment why (it is cross-crate shared, not genuinely dead).

Related clippy trap when moving long doc comments into a nested shared module:
`doc_lazy_continuation` treats a doc line starting with `+` (or `-` / `*`) as
a markdown list item, so a phrase like "(milestones 21b\n + 21c)" that gets
split across lines — harmless at the top level of a test file — becomes a
hard clippy error once the same comment lives one module deeper. Keep
list-like tokens on one line when re-flowing doc comments.

## Canonical paths are not cross-runtime command arguments

Windows `std::fs::canonicalize` can return a `\\?\` verbatim path. That is a
valid native Win32 path but not a portable argument for MSYS programs, which
interpret backslashes as escapes. Canonicalize only where identity/security
requires it; for a newly created Windows test scratch directory passed to both
native and MSYS children, retain the ordinary absolute path instead.

## Unix IPC may resolve only platform-owned aliases

Do not canonicalize arbitrary symlink ancestry while validating a Unix socket
runtime directory. A narrowly documented host alias such as macOS `/tmp` may be
resolved, but caller-created symlinks must fail as unsafe. Likewise, reject an
existing directory with group/other permission bits; never silently `chmod` a
caller-owned path to make an unsafe endpoint appear valid. Only directories
created by the adapter may be initialized at `0700`.

## Headless CI is not a desktop-service fixture

Linux adapter unit tests must not require the current host to run AT-SPI,
DBus, a compositor, or another desktop daemon. Split environment/proc parsing
into bounded pure helpers and test synthetic byte fixtures, including malformed
and missing values. Keep live service discovery best-effort in the adapter and
prove actual desktop integration only in a matching-host smoke environment that
explicitly owns that service.

## Ordinary CI must own Candidate's static workflow contracts

Do not let a release Candidate be the first lane that runs an integration test
which only parses workflow or release-policy source. Candidate may repeat that
contract, but the ordinary push quality job must run its named integration test
first. The same rule applies to deterministic build/task contract tests that
need no release artifact, such as target-pruning argument and source-order
checks. Keep command assertions exact enough to preserve semantic switches such
as `--ignored`; updating the production workflow and its parser assertion is
one coherent change. This prevents a cheap deterministic mismatch from wasting
the stress-inclusive qualification lane before failing.

## Linux AT-SPI publish must reconnect

A one-shot `serve()` then `pending()` dies with the first bus. If
`DBUS_SESSION_BUS_ADDRESS` points at a missing `unix:path`, hydrate must
replace it from the live AT-SPI process or `XDG_RUNTIME_DIR/bus` — filling
only when unset leaves a dead address in place. `start()` returns a handle
on first connect failure so the product keeps publishing snapshots;
`is_publishing()` is the live connection flag, not "this backend exists".
`retains_snapshots()` is how the product keeps a reconnectable handle and
still drops a no-op host. Do not require killing the con process to pick up
a replacement bus.

## AT-SPI action names are not required for a node click

Linux `GetActions` returns localized names. Chrome with
`--force-renderer-accessibility` commonly exposes `NActions >= 1` (often two
entries) whose names are empty strings, so a tree snapshot shows
`"actions":["",""]`. Structured `click --node` must still invoke AT-SPI:
prefer a named `click`/`press`, otherwise `DoAction(0)` (the spec default
action). Honor the boolean `DoAction` return. Do not refuse with
`a11y_action_unavailable` merely because names are blank, and do not require
the caller to pass `--coords` / `--degraded`. Focus stays named-`focus` then
`Component::grab_focus`; it must not fall through to the default click action.
Unit-test the index choice with synthetic name lists; prove actuation on a
live toolkit only in the owning smoke. When reading those empty names through
libagenterm's two-stage string ABI, a `cap==0` probe that reports
`required==0` is the empty payload — do not call again with `cap==0`, or
`buffer_too_small` will fail the whole tree.

## Named click without Action stays on the AT-SPI Component path

A showing named node may expose Component but not Action. Structured
`click --name` must not become `--coords`. Probe `GetInterfaces` for
`org.a11y.atspi.Action`; when it is absent, use `Component::GetExtents`
(screen) plus AT-SPI `DeviceEventController.GenerateMouseEvent` (`b1c`) at
the extent center. Reply `addressing` remains `accessibility-tree`. Fail
typed if extents are empty. Do not call XTest / input-inject from this
path. When `GetInterfaces` times out, still try `DoAction(0)` first (WebKit
`GetActions` hangs; `DoAction` often works), then Component only if the
Action interface is missing.

## Name addressing is wait-matching then the node path

`agenterm-cu click --name` / `agenterm-cu focus --name` must not grow a second actuation
backend. Resolve with the same showing/visible + case-insensitive
substring matcher as `wait --node-name-contains`, then call the existing
`--node` AT-SPI path. Require `--window`, reject `--name` combined with
`--node` or `--coords`, and return typed `a11y_node_not_found` on a miss.
Two or more showing/visible hits must return typed `a11y_node_ambiguous`
with the match count — never silently pick the first. The same uniqueness
rule applies to `wait --node-name-contains`. Never satisfy a name click
with a screenshot or degraded coordinates.

`agenterm-cu send-keys --name` is the same rule plus a native Device/key delivery:
resolve the unique showing node, then send the chord through AT-SPI
`DeviceEventListener.NotifyEvent`. A named showing node with no key
interface typed-fails (`a11y_key_unavailable`). That path never falls
through to XTest / `input_inject::send_keys`. A miss types nothing.
`send-keys --window HANDLE` without `--name` targets the same innermost
focused Text node `get-text --window` reads. Prefer
`DeviceEventListener.NotifyEvent` (`via=device-event`). con `Command`,
Chrome renderer entries, and WebKitGTK textareas do not close plain
typeable chords that way; plain typeable text (`314cGATE…` / `314GATE…`
/ `314bGATE…`, single letters) then uses the AT-SPI `EditableText` /
`Text` + toolkit set-value path (same as focused `send-text`) so
`focus --name X` → `send-keys --window H TEXT` → `get-text --window H`
closes without XTest. Live hosts: agenterm-con `Command` after
`focus --name` (`via=editable-text`, second con only — never steal the
resident control socket); Chrome `GetTextField` (`via=text`); Reasonix
composer `Message Reasonix…` under `scripts/reasonix-desktop-a11y.sh`
(eval helper, `via=text`). Special chords (`enter`, `ctrl+a`) without a
key interface still typed-fail. A synthetic `--window` with no focused
Text node typed-fails; it must not spray XTest.

`agenterm-cu send-text --name` resolves the same unique showing node, then writes
through native AT-SPI `EditableText` (`SetTextContents`, then `InsertText`)
when present. Chrome and WebKitGTK named fields expose `Text` (read) but
not `EditableText`. Chrome writes through AT-SPI `Text` plus the renderer
AX set-value. WebKit 2.52 never registers `EditableText` even on a
`<textarea id="composer-input">` (Reasonix composer); that write uses the
AT-SPI `id` / name attributes plus the eval helper loaded by
`scripts/reasonix-desktop-a11y.sh`, then is confirmed by `GetText`.
`GenerateKeyboardEvent` on X11 is XTest — do not use it as a silent
fallback. A named showing node that does not expose a writeable text
interface typed-fails (`a11y_text_unavailable`). That path never falls
through to XTest / `input_inject::type_text`. Explicit `--coords` or no
`--window` may still inject. `send-text --window HANDLE` without `--name`
is not that inject: it writes the same innermost focused Text node
`get-text --window HANDLE` reads, through `agt_a11y_node_set_text`.
`focus --name X` then `send-text --window H TEXT` then
`get-text --window H` must close the loop (`GetText == TEXT`). A
synthetic `--window` with no focused Text node typed-fails; it must
not spray XTest. agenterm-con named `Command` closes the same loop
through native `EditableText` (`via=editable-text`); launch a second
con on a private control socket (or none) — never steal
`unix:/tmp/run-box/agenterm-con.sock` / resident 62399. Chrome 151
still has no `EditableText`; the write is AT-SPI `Text` plus the
existing renderer AX set-value over that Chrome's own
`--remote-debugging-port`. The Reasonix composer (`Message Reasonix…`
under `scripts/reasonix-desktop-a11y.sh`) is the same verb: WebKit
2.52 has `Text` but never `EditableText`, so the write uses the
eval-helper set-value (`id=composer-input`) and proof is independent
`get-text --window` (no `--name`). Named `focus` on that textarea
must not call unbounded Action `GetActions` / `DoAction` — those hang
the same way click's `GetActions` does, and the outer 10s snapshot
deadline then fires as `a11y_action_timeout` before
`Component.grab_focus` runs. Bound the Action probe to
`ACTION_TIMEOUT` (250ms), then `grab_focus`. `click --name` also
sets the AT-SPI `focused` state if a caller already has that path. `DISPLAY=:2` box-chrome
defaults to 9224, which standing `chrome-profile-2` already owns on
`127.0.0.1` — a second window whose cmdline still says 9224 then
writes the wrong CDP tree (`no writable node named …`). Launch the
gate Chrome with `SAND_CHROME_REMOTE_DEBUG_PORT` on a free port.
Their payload argument is positional, so parse
`--` as the end of flags — otherwise text (or a chord) that starts with a
dash is eaten as a flag.

`agenterm-cu paste --name` is the clipboard form of that write. Resolve the unique
showing node, optionally seed the clipboard with `--text`
(`agt_clipboard_set_text`), then always read `agt_clipboard_get_text` and
write through the same AT-SPI `EditableText` / `Text` path. Do not
implement paste as Ctrl+V, XTest, `--coords`, or a screenshot. A named
showing node with no writeable text interface typed-fails
(`a11y_text_unavailable`). `matched.text` is still the resolve-time
snapshot. Linux X11 seed is a native CLIPBOARD `SetSelectionOwner` in
`adapters/linux/x11_clipboard.rs`, not `xclip` / `xsel`. A missing helper
is not `clipboard-unavailable` when `DISPLAY` is set. Do not drop a
one-off `/tmp/xclip` binary to unblock `paste --text`. WebKit/Reasonix
still uses the eval-helper set-value path; `wait --text-equals` must see
`GetText ==` the clipboard/typed string.

`paste --window HANDLE` without `--name` writes that same clipboard path
on the showing focused node — the same innermost `Text.GetText` candidate
`get-text --window` reads. Never XTest when `--window` is set. Proof is
independent `get-text --window HANDLE` (no `--name`) equal to the
clipboard string after `focus --name`. Live hosts: agenterm-con `Command`
after `focus --name` (`via=editable-text`, second con only — never steal
the resident control socket); Chrome `GetTextField`; Reasonix composer
`Message Reasonix…` under `scripts/reasonix-desktop-a11y.sh` (eval-helper
set-value, `via=text`, same as focused `send-text`). Optional `--text`
only seeds native CLIPBOARD; the field write always reads the clipboard.
Without `--window` paste is invalid.

`agenterm-cu copy --name` is the inverse read. Resolve the unique showing node,
read AT-SPI `Text.GetText` (`agt_a11y_node_get_text`), and publish that
UTF-8 through `agt_clipboard_set_text`. A named showing node with no
Text interface typed-fails (`a11y_text_unavailable`). Never XTest,
`--coords`, or screenshot. `matched.text` is the resolve-time snapshot
and does not count; the copied payload is independent GetText. A later
`paste --name` with no `--text` must be able to `ConvertSelection` that
CLIPBOARD. A CLI process that `SetSelectionOwner` and then exits leaves
CLIPBOARD unowned — `copy` therefore keeps a detached `agenterm-cu`
owner in the X11 selection loop (`AGENTERM_X11_CLIPBOARD_SERVE`) until
another owner takes it. Do not persist via `xclip` / `xsel`. A later
process must not treat a 1-byte `get_text` probe `TooLarge` as "no
clipboard text": `agt_clipboard_has_text` would then lie and
`agt_clipboard_get_text` would return empty, so `paste --name` without
`--text` writes nothing. Chrome fixture fields and the Reasonix
composer (`Message Reasonix…` under `scripts/reasonix-desktop-a11y.sh`)
share this path: after `send-text SRC`, `copy --name` reports
`via=gettext`, a different `send-text` clears the field, `paste --name`
with no `--text` rewrites SRC through the same WebKit eval-helper
set-value path as named `send-text`, and `wait --text-equals SRC` must
see independent GetText == SRC (not copy/paste/send `matched.text`).

`copy --window HANDLE` without `--name` copies that same GetText path
on the showing focused node — the same innermost `Text.GetText`
candidate `get-text --window` reads — onto native CLIPBOARD
(`via=gettext`). Never XTest when `--window` is set. Proof is
independent host circuit after `focus --name`: seed unique string →
`copy --window H` (no `--name`) → clear field → `paste --window H`
(no `--name` / no `--text`) → `get-text --window H` equals the seeded
string. Live hosts: agenterm-con `Command` after `focus --name`
(`via=gettext` on copy; paste restore `via=editable-text`; second con
only — never steal the resident control socket); Chrome `GetTextField`;
Reasonix composer `Message Reasonix…` under
`scripts/reasonix-desktop-a11y.sh` (`via=gettext` on copy; paste still
uses eval-helper set-value, `via=text`). Without `--window` copy is
invalid.

`agenterm-cu wait --text-equals` / `--node-text-equals` with `--name` is the
independent AT-SPI close-the-circuit after named `send-text` / `paste` /
`copy`.
Resolve the unique showing node, then call `Text.GetText`
(`agt_a11y_node_get_text`). Success is `ok:true` only when that GetText
equals the typed string. The `send-text` / `paste` reply's `matched.text`
is the resolve-time snapshot and does not count. A sidecar `agenterm-cu tree` walk
of snapshot `text` fields does not count. Timeout is typed `timeout` and
reports the last GetText. Never
screenshot, XTest, or `--coords`. Chrome AX set-value rides the window
PID's `--remote-debugging-port`; two Chromes sharing one CDP port can
make the write report success against another page while GetText on the
named node stays empty — that is why wait must observe GetText, not the
write reply. WebKitGTK/Reasonix has the same split: the eval helper
loaded by `scripts/reasonix-desktop-a11y.sh` returns `OK` when the JS
set-value is *queued* (worker self-report), not when `Text.GetText` on
the showing composer (`Message Reasonix…`) equals the typed string. Do
not treat helper `OK`, `last_text_write_via`, `send-text` / `paste`
`via=text`, or worker self-report as the wait hit.

`agenterm-cu wait --text-contains` / `--node-text-contains` with `--name` is the
same independent GetText poll with a substring predicate. Success is
`ok:true` only when that GetText contains `SUB`; published `text` /
`via=gettext` is still the full GetText, not the substring.
`send-text` / `paste` / `copy` `matched.text` does not count. Timeout is
typed `timeout` and reports the last GetText. Never screenshot, XTest,
or `--coords`. Do not implement contains as OCR, a sidecar tree walk, or
a check of the write reply.

`agenterm-cu --ssh <user@host>` is the first remote target tier (PRD 30).
It does not invent verbs: the host rewrites the abstract command to
`target=current` and runs a remote `agenterm-cu exec --json -` worker over
OpenSSH stdio (`ssh_transport`). Observe and actuate grants both forward;
desktop work still happens on the remote side (AT-SPI via that worker's
libagenterm). Get-selection evidence is loopback `sshd` plus a second
`agenterm-con` on a unique control socket: host
`send-text --window H --name Command -- SEED` (payload after `--`; not
`--text`) plants the seed, host `select --window H --name Command --start
N --end M` runs remote AT-SPI `Text.SetSelection`, then host independent
`get-selection --window H --name Command` returns that range
(`via=get-selection`; start/end equal the selected slice of the seed, or
the seed when the range is the whole field). Native AT-SPI
`GetNSelections` + `GetSelection`. Never screenshot, `--coords`, or XTest.
Missing Text typed-fails `a11y_selection_unavailable` on the remote worker
the same as local `current`. `get-extents` (3.29), `get-caret` (3.28),
`tree` (3.27), `focus` (3.26), `scroll` (3.25), `click` (3.24),
`set-caret` (3.23), `select` (3.22), `send-keys` (3.21), `copy` (3.20),
`paste --text` (3.19), and `send-text` (3.18) over ssh remain valid. Do
not steal `unix:/tmp/run-box/agenterm-con.sock` or kill the resident
avatar PIDs. Auth / connect failures are typed (`ssh_unavailable` /
`ssh_transport_failed`); missing `--ssh` on `--target ssh` is
`invalid_input`. Forward `DISPLAY` / `AT_SPI_BUS` / `AGENTERM_ABI_LIB`
via `--ssh-env` or host env (defaults copy common desktop keys when they
have no whitespace). Do not implement ssh as D-Bus port-forward or a
second control protocol in this cut.

`agenterm-cu --vnc <host[:port]>` is the first VNC target tier (PRD 30,
cuts 3.31 observe / 3.32 send-text / 3.33 paste / 3.34 copy / 3.35
send-keys / 3.36 select / 3.37 set-caret / 3.38 click / 3.39 scroll /
3.40 focus / 3.41 tree / 3.42 get-caret / 3.43 get-extents / 3.44
get-selection). It does not invent verbs: the host handshakes RFB
(security type None / `x11vnc -nopw` only in this cut), rewrites the
abstract command to `target=current`, and runs a local
`agenterm-cu exec --json -` session worker against the shared desktop
(`vnc_transport`; `DISPLAY` / `AT_SPI_BUS` via host env or `--vnc-env`).
Observe and actuate grants both forward; structured work still uses
AT-SPI / native clipboard on that session — never RFB framebuffer OCR,
screenshot, or `--coords`. Get-selection evidence is a **gate-owned**
loopback x11vnc (not the resident `:2` listener alone) plus a second
`agenterm-con` on a unique control socket and title: `Command` holds a
known ASCII seed and a known non-empty selection `START..END` (gate
precondition via already-landed `send-text` + `select`; not this cut's
verb), then host independent
`get-selection --window H --name Command` returns that range
(`via=get-selection`; native AT-SPI `GetNSelections` + `GetSelection(0)`;
`n == 1` and integer `start` / `end` equal the precondition range so
`seed[start:end] == expected`). Never screenshot, `--coords`,
mouse-drag, RFB framebuffer OCR, or a cached setter reply. Missing Text
typed-fails `a11y_selection_unavailable` on the session worker the same
as local `current`. `get-extents` (3.43), `get-caret` (3.42), `tree`
(3.41), `focus` (3.40), `scroll` (3.39), `click` (3.38), `set-caret`
(3.37), `select` (3.36), `send-keys` (3.35), `copy` (3.34),
`paste --text` (3.33), and `send-text` (3.32) over vnc remain valid. Do
not steal `unix:/tmp/run-box/agenterm-con.sock` or kill the resident
avatar PIDs. Connect / protocol / auth failures are typed
(`vnc_unavailable` / `vnc_transport_failed` / `vnc_auth_failed`);
missing `--vnc` on `--target vnc` is `invalid_input`. Do not implement
vnc as a second control protocol or D-Bus port-forward in this cut.

### Per-target `capabilities` (cut 3.47)

`capabilities` is the existing observe verb only — no `targets` enumeration
and no new command. Two distinct facts must not be conflated:

1. **Public tier / transport** owned by CU (`current` in-process, `ssh`
   OpenSSH exec, `vnc` RFB session worker, `rdp` placeholder).
2. **Mechanism status** owned by the `current` worker / libagenterm.

SSH/VNC rewrite the command to `target=current` for the worker. Restoring
only `reply.target` is not enough for `capabilities`: the worker payload
also carries `data.target:"current"` and often
`transport.status:"in_process"`. Normalize both `reply.target` and
`data.target` to the public tier, keep the worker identity as
`worker_target` / `worker_transport`, and set the public `transport`
(`openssh_exec` / `rfb_session_worker`). Otherwise callers see a ssh/vnc
request that claims `data.target:"current"`.

RDP is the static exception: `capabilities` succeeds with
`transport.status:"placeholder"`, `available:false`,
`reason:"rdp_unavailable"`, and `verbs.tree` unsupported — **zero** DNS
or TCP to the endpoint. Every other RDP verb stays fail-closed
`rdp_unavailable`. Discovery still requires the observe grant (`refused`
without it) and grants no actuation right. Never declare live RDP or
unproven macOS AX as available from this path.

`cu scroll --name` is one-shot AT-SPI `Component.ScrollTo(TopEdge)`
(`agt_a11y_node_scroll`). Success is `ok:true` / `via=scroll-to`.
Missing / false / `UnknownMethod` typed-fails
(`a11y_scroll_unavailable`). ScrollTo true with no later independent
geometry change is `a11y_scroll_no_effect`, not `timeout` — this is not
a wait poll. Never Action `scroll*`, XTest wheel, `GenerateMouseEvent`,
or `--coords`. `matched.extents` / snapshot `node.bounds` do not count.
`cu get-extents --name` is the independent `Component.GetExtents(Screen)`
observe sibling (`agt_a11y_node_get_extents`, same `CoordType::Screen`
as `invoke_component_click`). Empty extents (w/h <= 0 or call fail)
typed-fail (`a11y_extents_unavailable`). Both verbs are single-node
`NODE_TIMEOUT` calls. Never fill snapshot bounds during a tree walk —
WebKitGTK hangs Component on walk. con publish implements
`Component.ScrollTo(TopEdge)` by applying a persistent y offset to the
named `OffscreenField` Session child (layout snapshots stay
unscrolled). Independent `GetExtents` is the proof; do not treat
snapshot `node.bounds` as movement.

WebKitGTK `Component.GetExtents(Screen)` works as a single-node call
(snapshot `bounds` stay `0,0,0,0`). `Component.ScrollTo` returns true
without changing those extents. GetChildren under the embed already
returns the unique owner (`:1.N`), not
`org.webkit.*.Sandboxed.WebProcess-*` — `open_bus_object` GetNameOwner
does the same. Route scroll by: well-known dest, unique dest that owns
that well-known name, or toolkit `WebKitGTK`. When
`scripts/reasonix-desktop-a11y.sh` loaded the eval helper, `scroll
--name` applies `scrollIntoView({block:'start'})` on the GTK thread
(AT-SPI `id` + accessible name; same socket as set-value, hello
`A11YSCROLL1`). Chrome has no helper socket and keeps native ScrollTo.
Do not treat helper `OK` as geometric proof — only a later independent
`get-extents` `|Δy|`.

`cu select --name --start N --end M` is one-shot AT-SPI
`Text.SetSelection(0, start, end)` (`agt_a11y_node_set_selection`).
Success is `ok:true` / `via=set-selection`. Missing Text /
`UnknownMethod` typed-fails (`a11y_selection_unavailable`).
SetSelection false is `a11y_selection_no_effect`, not `timeout` — this
is not a wait poll. Never XTest, mouse-drag, `GenerateMouseEvent`, or
`--coords`. The `select` reply (including echoed `start`/`end`) is not
proof. `cu get-selection --name` is the independent
`GetNSelections` + `GetSelection(0)` observe sibling
(`agt_a11y_node_get_selection`). `n == 0` is empty success. Do not
implement select as a coordinate drag or a screenshot crop.

Chrome text fields expose `Text.SetSelection`. A range such as `0..4`
on a named field with known contents (`SelectField` / `HELLO`) is the
live check: independent `get-selection` before is empty or not that
range; after `select` it must be `start=0,end=4`. Unfocused fields
often report `n==0`; grab-focus then `SetSelection(0,…)`, and only if
that returns false with `n==0` use `AddSelection` to create selection
0. Still AT-SPI Text, never a mouse drag.

WebKitGTK 2.52 / Reasonix composer (`Message Reasonix…` under
`scripts/reasonix-desktop-a11y.sh`) already implements those same
`Text.SetSelection` / `GetNSelections` / `GetSelection` methods. Unlike
`ScrollTo` (true-no-op) and `EditableText` (absent), select /
get-selection need no eval-helper glue and must not grow an
`A11YSELECT1` hello. Independent `get-selection` after `0..4` is
`n=1 start=0 end=4`. Do not treat the `select` reply as proof.

con publish implements those same Text methods on named `Command`.
Layout snapshots replace node text; the publisher stores the range
separately (same persistence pattern as `scroll_dy`). A collapsed
range (`start == end`) is `n=0`. Product `SetText` / insert /
backspace clear the stored range; `Ctrl+A` sets `0..len`. Independent
`get-selection` is the proof. Do not treat `SetSelection` true as
proof, and do not implement select as a mouse-drag.

con publish also implements `Text.SetCaretOffset` / `CaretOffset`
(`GetCaretOffset`) on named `Command`. The previous stub returned
true from `SetCaretOffset` and always reported `character_count` for
`CaretOffset`, so independent readback never matched a typed offset.
Store the caret beside the selection map; layout snapshots must not
drop it. A negative offset is false; an offset past the end clamps.
`SetCaretOffset` collapses the stored selection; `SetSelection` moves
the caret to `end`. Independent `cu get-caret --name` after
`cu set-caret --name --offset N` is the proof. Do not treat
`SetCaretOffset` true as proof, and do not implement caret as
`--coords` / XTest.

Chrome named text fields expose those same `Text.SetCaretOffset` /
`CaretOffset` methods. Zero protocol change: the ABI 1.9 verbs and
Linux adapter already used for con work on a
`--force-renderer-accessibility` Chrome `<input aria-label>` (live
fixture `CaretField` / `HELLO` in `fixtures/cu/310-chrome-caret.html`).
An unfocused field often reports `CaretOffset=-1`; after
`set-caret --offset 2` (which grab-focuses) independent `get-caret`
is `2`. Do not add an eval helper or CDP/`--coords` fallback. No
`A11YCARET1` hello. The `set-caret` reply is not proof.

WebKitGTK 2.52 / Reasonix composer (`Message Reasonix…` under
`scripts/reasonix-desktop-a11y.sh`) already implements those same
`Text.SetCaretOffset` / `CaretOffset` methods. After `send-text HELLO`
independent `get-caret` is `5`; after `set-caret --offset 2`
independent `get-caret` is `2`. Unlike `ScrollTo` (true-no-op) and
`EditableText` (absent), caret needs no eval-helper glue and must not
grow an `A11YCARET1` hello. The `set-caret` reply is not proof.

`Text.GetText` is the same native-Text story: con `Command`, Chrome
`GetTextField`, and the Reasonix composer all expose real GetText
(ABI 1.9 `agt_a11y_node_get_text`), so one-shot `get-text --name`
reads it independently of any write — `send-text` / `paste` / `copy`
`matched.text`, `last_text_write_via`, the WebKit eval helper queued-job
`OK`, and tree snapshot `text` are not proof. After the field holds
`HELLO`, independent `get-text` must equal `HELLO` (`via=gettext`).
Do not add an eval helper or CDP/`--coords`/XTest get-text path. No
`A11YGETTEXT1` hello.

## Do not drop the AT-SPI bus between resolve and keys

Linux `AccessibilityConnection::new()` is not a cheap handle. An `agenterm-cu`
process that opens one connection for the tree snapshot, drops it, opens
another for `grab_focus`, drops that, then injects XTest `Home` into
Chrome's omnibox leaves the renderer accessibility tree empty (and can
crash Chrome). The next `send-keys --name` then returns
`a11y_node_not_found` or `accessibility-tree mechanism unavailable`.
Keep one process-wide connection, clone it for each AT-SPI call, and do
not shut down at-spi-bus-launcher / registryd. Do not let the tokio
runtime or the last zbus connection Drop during process teardown — that
abort path crashes Chrome's renderer tree. Leak both and let the OS
close the socket. After named `send-keys` / `send-text`, keep that
connection pumping for a short bounded drain so Chrome can emit caret
events before the process exits; exiting immediately after XTest `Home`
closes the socket under those events and the next named command sees
`a11y_node_not_found`. Prove two named `send-keys` ~1s apart plus
`tree --window` still reporting 100+ Chrome nodes on a live `DISPLAY`
host; unit tests must not require that bus.

## Honor `AT_SPI_BUS`; strip `GetAddress` guids

Host live gates set `AT_SPI_BUS=unix:path=$XDG_RUNTIME_DIR/at-spi/bus_N`.
cu used to ignore that name and call `org.a11y.Bus.GetAddress`, which
returns the same path plus `,guid=…`. Later `dbus-daemon` /
`at-spi-bus-launcher` processes can reuse that unix path without being
killable; the guid then names a dead owner, `select_roots` is empty, and
`tree --window` synthesizes a one-node X11 `frame` (`tree_n=1`) that is
not Chrome's renderer. Prefer `AT_SPI_BUS_ADDRESS` then `AT_SPI_BUS`,
connect to the path without the guid, and skip the session-bus hop when
either env is set. `scripts/box-chrome-a11y.sh` must also export
`AT_SPI_BUS_ADDRESS` and write `$XDG_RUNTIME_DIR/at-spi/bus` after
box-chrome rewrites XDG to `/tmp/xdg-runtime-box-$DISPLAY` — atk-bridge
looks for the file named `bus`, not `bus_2`. Do not pkill at-spi to
"fix" a shared socket. Unit-test the address normalizer on synthetic
strings; do not require a live registry.

## Window matching is more than `_NET_WM_PID`

Linux `tree --window` must not treat D-Bus connection PID equality as the
only window↔AT-SPI join. WebKitGTK (Wails/Reasonix) embeds its document
tree under a **well-known** bus name
(`org.webkit.app-*.Sandboxed.WebProcess-*`). The atspi `ObjectRef` type
only deserializes unique names (`:1.47`), so `GetChildAtIndex` /
`GetChildren` typed as `ObjectRef` drop that child and the scoped tree
stops at the GTK frame. Read children as raw `(String, ObjectPath)`,
resolve well-known names with `GetNameOwner`, and keep walking.

Do not open that tree through `AccessibilityConnection::new()`. atspi
0.30's default P2P path (`GetApplicationBusAddress` plus a unix-socket
handshake per registry child) hangs on WebKit/Wails sockets, so `agenterm-cu tree`
dies with `a11y_tree_timeout` and never reaches named document widgets.
Connect to the a11y bus only. Skip dests with no owner (a dead web
process leaves a filler stub). WebKit `GetRoleName` is often empty —
use `GetRole` (43 = button). Snapshot only Accessible name/role/state;
`GetActions` / `proxies()` introspect hang per node and blow the 10s
deadline. Named `click` invokes AT-SPI `DoAction(0)` only after a
bounded `GetActions`; named `focus` must bound that same Action
probe and then `Component.grab_focus`, or Reasonix composer
`focus --name` times out before the textarea reports `focused`.

Match a window to application roots by, in order: the window's
`_NET_WM_PID`, descendant PIDs (`/proc/*/status` PPid), then exact
normalized equality of the X11 title / `WM_CLASS` / `comm` against the
application or frame name. Do not substring-match titles (that pulls
Chrome into an unrelated window).

A custom-raster toolkit (winit/softbuffer `agenterm-con`) is not GTK and
does not load `atk-bridge`, so the AT-SPI registry never sees it as an
application. `agenterm-cu tree --window` then used to emit only a one-node X11
title `frame`. That fallback is window identity, not a widget tree: named
`click`/`focus`/`send-text` cannot address the composer, SEND button, or
session. Linux `agenterm-con` now publishes those children through the
platform `a11y-publish` AT-SPI server (Accessible + Component + Action,
plus Text/EditableText on the composer and a named `OffscreenField`
Session child whose `Component.ScrollTo` moves `GetExtents`).
Registered with `Socket.Embed`. The one-node X11 frame remains only for
toolkits that still do not register (`xfce4-terminal` without
atk-bridge). Unit-test the published chrome snapshot without a bus;
prove `tree --window` `n>=5` and named actuation on a live `DISPLAY`
host. Do not treat the one-node frame as the success path for con.

A single-host capability still may not reach product code as
`#[cfg(target_os = ...)]`. The boundary suite scans `crates/agenterm-con/src`
too, and the subsystem-entrypoint exemption covers only the windows-subsystem
attribute, so an OS `cfg` in `main.rs` reddens the quality lane. Publish the
facade unconditionally, let `selected.rs` pick a no-op backend off the host, and
keep the heavyweight dependency edge in `[target.'cfg(...)'.dependencies]` --
Cargo manifests are outside the scan and are the supported place to buy the real
implementation on one target only. Give the facade a capability predicate
(`is_publishing()`) so callers skip snapshot work without asking which OS they
are on. This also removes the second failure mode of the `cfg` pair: the
`#[cfg(not(...))]` stub method has no caller on that host, and `-D warnings`
turns `dead_code` into a build error only on that one target cell.

The same `target_os` ban applies inside `crates/agenterm-platform/src/*.rs`
facades. `selected.rs` and `adapters/**` are the only legal OS-selection
sites; a one-line Linux probe such as `last_text_write_via` still has to
live on the selected module, with the off-host stub returning the documented
default. Platform adapters also cannot read `AGENTERM_*` environment names
(`platform_crate_has_no_agenterm_product_dependency_or_source_coupling`).
Use `PLATFORM_*` (already used for IME). Product launchers and LD_PRELOAD
helpers must read and export that same `PLATFORM_*` name, not `AGENTERM_*`.

## File existence is not writer completion

For a synchronous writer running on a test driver thread, another thread must
join that driver (and inspect its typed result) before decoding the output.
Polling `Path::exists()` races the interval after file creation but before the
encoder has completed and can surface valid in-progress output as
`UnexpectedEof`. Atomic product publication may use complete-file visibility as
its contract; a direct native test helper without that publication boundary may
not borrow the same assumption.

## `#[cfg(unix)]` is not one library name

A test gated `all(unix, target_arch = "x86_64")` still runs on macOS x86_64.
`libc.so.6` exists only on Linux; macOS needs `libSystem.B.dylib`. CI run
`31953163587` job `agenterm / osx-x86_64` failed `exec_base` on that dlopen
after the dyn exec-base merge. Pick the soname from `target_os`, not from
"unix".

## Native consumers must match the Rust target ABI

Do not select a C or C++ compiler merely because it appears first on the host
`PATH`. Derive the compiler family from Rust's `target_env`: an MSVC artifact
must use the discovered MSVC toolchain, while a GNU artifact may use the GNU
family from `PATH`. This applies to both dynamic and static consumer probes;
mixing MinGW with an MSVC Rust library produces misleading unresolved runtime
and system symbols rather than evidence about the public ABI. Print the chosen
target environment and compiler in test logs so CI selection is auditable.

## C++ consumer probes: no `/TP`, and ASCII-only generated sources

Two measured MSVC failure modes when a test compiles a real `.cpp` consumer
against the shipped library (milestone 62, `tests/cpp_consumer.rs`):

- **Do NOT pass `cl /TP`.** cl.exe compiles a `.cpp` as C++ by suffix alone.
  `/TP` instead forces EVERY input file to be treated as a C++ source, so the
  `.lib` link inputs (`agenterm.dll.lib`, `agenterm.lib`, `ws2_32.lib`, ...)
  are handed to c1xx as sources: C2220/C4819 noise on the first `.lib`, then
  `C1083: cannot open source file: 'ws2_32.lib'` for the rest. The C-side
  consumers never needed `/TP` and the C++ side must not add it either; the
  `.cpp` suffix is the mode switch.
- **Generated sources must be pure ASCII.** MSVC reads source in the host
  code page (936 on zh-CN CI/locales); any non-ASCII byte (e.g. an em dash in
  a comment) triggers C4819 "file cannot be represented in the current code
  page", which `/WX` escalates to C2220. Keep every string written into a
  generated `.c`/`.cpp`/`.inc` ASCII-only (use `--` instead of `—`), even
  when the generating Rust source itself is UTF-8.

The same guard-coverage idea as the C symbol-presence gate works from C++:
generate (from `exports.txt`) an address table of all exports via
`reinterpret_cast<void (*)()>(name)` — never a call — and iterate the WHOLE
table so the linker must resolve every name. Link success = the `extern "C"`
guard unmangles all of them; commenting the guard out must turn that link
into `LNK1120: N unresolved externals` with mangled names (`?agt_*@@...`).
That negative proof is the only evidence the guard actually does something.

## Close may wake a reader with buffered data before EOF

For pipes, PTYs, and stream-like native handles, a cross-thread close contract
should assert bounded wakeup, not that the first completed read is EOF. Bytes
written before close may already be buffered and are a legal successful read;
the reader may observe EOF or a typed I/O failure only after consuming them.
Use a deadline to prove close cannot leave the read blocked, accept buffered
success within the buffer bound, and test eventual termination separately when
that stronger behavior is part of the public contract.

## Two-stage native enumeration must tolerate bounded growth

For process, window, environment, or other live-table FFI enumeration, a
successful size probe does not freeze the collection. Between probe and fetch,
the required count may grow. Consumer tests must retry only when the fetch
returns the documented insufficient-capacity status and a strictly larger
required count, with a small fixed attempt bound; all other failures remain
immediate errors. Requiring the first fetch to succeed turns normal host churn
into flaky ABI evidence, while an unbounded retry can hide a nonconvergent or
malicious provider.

## GUI control readiness is not child-process readiness

A black-box GUI test that can reach the control endpoint has proved only that
the host accepts commands. The initial PTY child may still be starting. Do not
race a marker emitted from process-launch arguments against control discovery.
After the control endpoint is ready, inject a marker through the public input
interface and wait for that marker through the public observation interface;
buffered PTY input then provides the rendezvous with actual child readiness.

## Bound both queue cardinality and ownership duration

A fixed-capacity pending-request queue is not operationally bounded when each
caller can choose an effectively infinite timeout. Validate a product maximum
before transferring the reply or resource owner into the queue. Rejection must
leave ownership with the normal dispatch error path and register no latent
entry, so a small number of hostile or mistaken requests cannot exhaust the
control surface indefinitely.

Bound aggregate state at the shared mutation primitive, not only at an external
read boundary. Repeated individually bounded clipboard, IPC, keyboard, or IME
payloads can otherwise grow one buffer without limit. Byte ceilings for UTF-8
text must truncate at a character boundary.

Stable-id allocators must use checked progression before mutating related
collections. Capacity, id exhaustion, or duplicate insertion must leave tree,
active-selection, and owned-resource stores aligned; a `debug_assert!` is not a
release-build uniqueness contract.

An asynchronous completion boundary must cover synchronous fallback delivery
as well as the normal worker path. Initialization failure, queue-full, and
disconnected-worker branches often invoke the same user callback on the caller
thread; route every branch through one panic-contained completion helper so a
fallback cannot unwind into GUI or FFI dispatch.

Accessibility and automation callbacks are external event producers even when
they run in-process. Cross into the GUI through a fixed-capacity FIFO, drain a
fixed per-turn budget, wake producers only on the empty-to-nonempty transition,
self-wake for backlog, and expose pending/drop counters. An unbounded
`Mutex<Vec<_>>` turns a bus flood into both memory growth and an unbounded
event-loop callback; waking for every rejected item preserves a CPU flood even
after memory is bounded.

When an OS-facing adapter keeps an optimistic mirror, its callback must return
whether the product accepted ownership. Commit mirrored text, focus, or action
state only after acceptance; a fire-and-forget handler plus a bounded product
queue otherwise reports success and permanently diverges when saturation drops
the event.

Count-bounded queues are not memory-bounded when events own strings or blobs.
Enforce both a per-item payload ceiling and an aggregate queued-byte ceiling,
account bytes before ownership transfer, and return the exact allowance when a
batch drains. Expose queued bytes alongside item/drop counts.

## Saturate native geometry before narrowing coordinates

Treat window dimensions, DPI scales, pointer coordinates, and row indexes as
hostile numeric inputs. Clamp `NaN` explicitly, use saturating coordinate
arithmetic, and perform checked narrowing before multiplying indexes or scaled
values; an ordinary comparison does not constrain `NaN`, and unchecked casts
or products can collapse or panic at native callback boundaries.

## Raw-handle field widths can vary by compilation target

A variant in a cross-platform raw-handle enum can expose a different integer
width on different targets even when the match arm is shared source. Do not
choose one conversion from the host build alone. Use target-compiled `cfg`
branches when one target has a provably infallible conversion and another
requires a checked conversion, then compile both target cells. This avoids
both silent narrowing and a Clippy fix that fails to type-check elsewhere.

## Product executable names must not be occupied by ABI demos

When a package has one accepted product executable, its real command shell and
desktop host own that executable name. A dynamic-library smoke/demo belongs in
a test, example, or diagnostic subcommand; it must not create a second product
binary or take the formal executable name while the real product ships under a
short alias. Build scripts and black-box tests must name the formal executable
explicitly so an accidental extra `[[bin]]` cannot become a release artifact.

## Desktop-host ABI keeps mechanism and product meaning separate

The platform/dynamic-library boundary owns registration, event transport and
resource cleanup; the product owns action IDs, labels, shortcut choices and
what each action means. Keep these recurring rules together:

- `action_id == 0` means no event. Product actions must use nonzero IDs.
- Open, poll and close execute on the same owning thread. A native message loop
  or registration handle is not safely transferable merely because its Rust
  wrapper is movable.
- One shortcut conflict or duplicate action must return a typed failure without
  corrupting cleanup. Track each successfully acquired icon, window and hotkey
  independently, and release exactly that acquired subset on rollback/close.
- The ABI must not embed a placement catalog, Quit policy or other product
  semantics. It transports opaque numeric action IDs; `agenterm-cu` assigns
  their meaning.
- Give every menu, global-shortcut and native callback one product-owned
  `action_id -> Command -> Executor` function. Black-box self-test should call
  that exact function with insufficient authority and require a typed refusal:
  this proves dispatch without moving the user's window, and catches a host
  path that silently reimplements command meaning or bypasses authorization.

## Hold one audit sink across an authorized side effect

Opening an audit path once for the pre-action record and again for the outcome
creates a race: the first append can succeed, the mechanism can actuate, and a
second open can fail while the caller is still told the action succeeded. Open
and retain one writable sink before dispatch, append and flush `attempt` before
the side effect, then append and flush the typed outcome through that same
handle. A pre-action failure must prevent dispatch. An outcome-write failure
cannot undo an already completed native action, so return `audit_unavailable`
and preserve the original mechanism reply as diagnostic context instead of
silently discarding the audit error. Inject append/flush failures through a
test-only sink or constructor; do not mutate a process-global audit-path
environment variable in parallel tests.

## Publish small persisted state without erasing the last valid snapshot

Treat a missing state file as empty state, but report malformed JSON or invalid
cursors/capacities as corruption; silently replacing either with defaults loses
the evidence needed to diagnose the failure. Write a collision-safe
`create_new` temporary beside the destination, complete `write_all`, `flush`
and file sync, close it, then publish with one replacement rename. RAII must
remove every abandoned temporary. A same-directory rename owns name atomicity;
directory sync is a separate durability claim and is not available with equal
strength on every host.

Give tests an injected path plus write/publish fault seams. Prove partial-write
cleanup, failed-publication preservation, validation of corrupt input, and a
successful second replacement/reopen—not only first-file creation. Keep this
file transaction distinct from native side effects: atomically replacing JSON
does not make a preceding window move or other OS action transactional. If the
product requires all-or-nothing behavior across both, it needs an explicit
prepare/commit or compensation contract and failure evidence for that boundary.

Bounded one-shot authority must be durably reserved before the authorized
side effect and must not be refunded merely because the downstream mechanism
fails; refunding makes a failed attempt replayable. Validate target/session,
scope, revocation, time bounds and remaining uses before cloning and publishing
the next store generation. A generation comparison without a cross-process
lock detects an already-published conflict but does not close the
compare-to-rename race between two processes, so document that boundary and do
not call it atomic authorization. Close that race for cooperating writers with
a stable sibling lock sidecar: take a non-blocking cross-process lock, re-read
the generation while holding it, and retain the guard through replacement,
parent sync and the in-memory commit. Never lock the replaceable JSON itself;
Unix `flock` follows the opened inode, so a rename would let a new opener bypass
the old guard. The sidecar must remain stable and must not be deleted or
replaced while the store is live. This does not protect against non-cooperating
writers, hostile directory mutation or filesystems without coherent local lock
semantics. On the pinned Windows Rust toolchain,
`std::fs::rename` already requests replace-existing semantics; a manual
destination-to-backup swap adds a crash interval and must not be used as an
"atomic" fallback.

Authorization selectors and provider material must not inherit through a
transport worker's generic environment forwarding. Reserve the complete
case-insensitive product prefixes (for example `AGENTERM_CU_GRANT*` and
`AGENTERM_CU_AUTH*`) and reject them before any process spawn, network
handshake or mechanism call. A future remote authorization handoff needs an
explicit one-command delegation envelope bound to command, target and expiry;
raw scope strings or environment variables are not delegation.

Treat routing and authorization identity as different types. A target enum,
hostname, IP, account, port, PID, native window handle or display name can
locate work but cannot identify the provider plus exact desktop session for a
persistent grant. Put opaque target/session IDs behind a sealed verified
provider, expose no arbitrary-string constructor, and fail closed when the
provider or session proof is unavailable. A placeholder transport remains
unsupported even if some caller offers identity-shaped data.

Separate installation identity enrollment from ordinary load/query. Enrollment
may exclusively create one random key while holding a stable key sidecar lock;
load and query must never replace missing, short, corrupt, linked or
permission-unsafe state. Re-read and compare a newly published key before
deriving its provider ID. A session binding must combine that provider with
native login and interactive-desktop facts, and it must be resolved again at
the side-effect boundary rather than treated as a process-lifetime snapshot.
On Windows, token SID plus authentication/session IDs are insufficient alone:
require a positive active WTS session with logon time and prove the caller is
attached to the input desktop. Treat the domain-separated digest as an opaque
equality identifier, not a MAC or credential, and report unsupported on peers
whose equivalent session proof has not been implemented.

When a persisted authorization format predates verified identity, version the
trust boundary rather than only the JSON shape. New grant specs and attempts
must be constructed from the sealed binding type, and the stored record must
carry and validate that binding version plus its exact canonical encoding. A
legacy record with caller-provided target/session strings cannot be silently
migrated by adding a field or prefix: reject it typed, preserve its bytes, and
require an explicit migration flow that obtains fresh identity proof.
Keep the production store opener separate from the raw injected-path seam:
resolve machine-local product data once, require an explicit parent directory,
protect it before reading or writing authority state, reject link-like store
entries, and create every replacement temporary with the platform's private
exclusive-create options rather than ordinary umask-dependent defaults.

For a persisted authorization attempt, keep the order explicit and testable:
open the audit, resolve the verified binding, durably reserve the use, flush an
attempt record, resolve the binding again immediately before dispatch, then
write the outcome with the same decision ID. A reservation is not refundable
after an audit or mechanism failure. If the binding disappears or changes
after reservation, record a failed no-dispatch outcome; if reservation
publication reports uncertain durability, return authorization-in-doubt and do
not execute or retry. Never put the session digest or installation key in the
audit merely to prove the comparison occurred.

For window placement, compensation is a saga, never an atomicity claim. Read
the exact native bounds, revalidate handle plus process/application identity,
apply, independently read back the final rect, then publish cloned history.
Only roll back a history-publication failure when the current identity and rect
still equal that transaction's known successful readback. If the native apply
itself failed after a possible partial move, its last owned rect is unknown;
do not overwrite a stable-looking rect that may belong to a concurrent user
move. Return structured `possibly_applied` / `in_doubt` instead. A rollback
must itself be read back exactly, and failures after a successful rename must
distinguish published-but-durability-uncertain state from an unpublished file.

## Windows UIA clients keep identity, apartments and actuation separate

The Windows accessibility adapter established a reusable native-FFI rule set.
Five pure tests and two real Win32 UIA fixture tests cover the adapter. The
staged public `cu-windows-smoke` also passes all seven declared host, DLL,
window-identity, tree, name-actuation, value-wait and cleanup receipts;
Candidate and release status remain separate and are not implied.

- Initialize COM at the operation boundary with
  `CoInitializeEx(COINIT_MULTITHREADED)`. If the calling thread already owns a
  different apartment (`RPC_E_CHANGED_MODE`), borrow that apartment without an
  unmatched `CoUninitialize`. Keep every COM interface, BSTR, SAFEARRAY and
  VARIANT operation-local and RAII-owned; never cache a COM pointer across
  threads, calls or apartments.
- Configure `IUIAutomation2.SetAutoSetFocus(FALSE)`, connection timeout and
  transaction timeout before traversal, then enforce an independent wall-clock
  budget and hard node, depth, sibling, RuntimeId and string limits. COM's own
  timeout is not a substitute for a bounded caller.
- Serialize a node's RuntimeId path as identity, not ownership. For every
  Value, Invoke, Focus, text or key request, start from the supplied HWND (or a
  deliberately bounded desktop root), walk again and compare every RuntimeId
  segment. A missing window, denied call, timeout or changed/recycled node must
  become a typed failure instead of using a stale interface.
- Keep product `Command`/`Executor` semantics above the dynamic-library and
  platform boundary. The platform adapter owns UIA `SetFocus`, Value/Text and
  Invoke/SelectionItem/Toggle/legacy patterns; it must not choose targets or
  reinterpret actions for the product.
- Structured actuation must remain structured. If the required UIA pattern is
  absent, fail typed; never hide a coordinate click behind UIA success. Key
  delivery may focus through UIA and then call the platform input mechanism,
  but its result must state that route explicitly, such as
  `uia-focus+send-input`.
- A two-stage native enumeration is not a stable snapshot. After querying
  `required` and allocating `capacity`, the desktop can gain windows before the
  fill call. Treat `required > capacity` as bounded-retry churn: discard the
  partial result, query/allocate again, cap attempts and return typed failure on
  exhaustion. Never truncate while claiming success, write past capacity or
  retry forever.
- A unit test that sends a synthetic HWND or RuntimeId through the selected
  native backend must assert the typed failure class, not one host-specific
  error code: Windows may reject the identity as invalid before lookup, a stub
  host may report unsupported, and an unstaged unit-test process may have no
  adjacent runtime dynamic library. A real desktop can also drop the synthetic
  or enumerated window before traversal, so `a11y_window_gone` is a valid
  pre-actuation typed failure; the test must still require failure and prove no
  input, clipboard, selection or geometry side effect. Keep exact matching
  semantics in pure tests and prove native success with an owned fixture or
  public smoke journey.

## macOS Accessibility trust is signature + process, not the Settings label

Proven on the `agenterm-cu host` / `AgentermCu.app` host (`scripts/install-cu-hotkeys.sh`,
`crates/agenterm-cu` ax_guide / status_menu / hotkeys).

### What actually gates AX

- Settings → Privacy → Accessibility shows a **name**. Runtime
  `AXIsProcessTrusted()` checks whether **this process** matches TCC for the
  **current code requirement**.
- Ad-hoc `codesign --force --sign -` changes the designated requirement to a
  bare **cdhash**. Rebuild/reinstall without a fresh grant leaves Settings
  showing ON while the host logs `ax_trusted=false` and hotkeys fail with
  `ax_api_disabled`.
- Measured failure mode: Settings ON, TCC `com.agenterm.cu` csreq
  `cdhash H"05f4…"`, running binary `cdhash H"f61f…"`, launchd
  `ax_trusted=false`. After `tccutil reset` + reinstall + user enable once
  with matching csreq, launchd reported `ax_trusted=true` and Carbon hotkeys
  applied placements.

### CLI success is not host success

- Terminal/IDE-spawned `agenterm-cu window-place` can succeed while the LaunchAgent is
  untrusted. TCC **responsible process** lets the CLI borrow Terminal's grant.
- The LaunchAgent is responsible for **itself**. Accept only evidence from the
  launchd-hosted process: `~/.local/share/agenterm/ax-status` (`trusted=1`),
  log line `ax_trusted=true`, or a real hotkey move — not a CLI place from
  this shell.

### Install / verify contract

- Install into `~/Applications/AgentermCu.app`, `lsregister -f`, LaunchAgent
  with `AssociatedBundleIdentifiers` = `com.agenterm.cu`.
- After every re-sign: `tccutil reset Accessibility com.agenterm.cu` so the UI
  cannot keep a stale ON. User enables **AgentermCu** once for the new
  signature. Ignore or remove the old path entry `agenterm-cu` (CLI symlink);
  it is not the hotkey host.
- At start: write `ax-status`, log `ax_trusted=…`, optional one-shot
  `AXIsProcessTrustedWithOptions` + open Accessibility. Build the prompt
  options with `NSDictionary`/`NSNumber` — a function-local
  `kCFBooleanTrue` + null-callback `CFDictionaryCreate` SIGSEGV'd in
  `CFGetTypeID` / `AXIsProcessTrustedWithOptions`.
- On `ax_api_disabled` after a real grant, exit non-zero once so KeepAlive
  (`SuccessfulExit=false`) restarts into a process that can read the new
  grant. Do not claim the switch is fine while `ax-status` says `trusted=0`.

### Product UX for the host

- Menu bar extra only. Refresh the first item in `menuWillOpen` (status + open
  Settings). No popup card, no timer that reopens Settings or
  `activateIgnoringOtherApps` (that steals the click needed to flip the
  switch).
- No background TCC poll. Humans discover trust when they open the menu or
  press a hotkey.

### macOS AX `current tree` (PLACEHOLDER cut 3.45)

- Adapter: `crates/agenterm-platform/src/adapters/macos/accessibility_tree.rs`,
  selected only under `cfg(target_os = "macos")` + feature `a11y-tree`.
  Backend string is `"ax"`. Product command stays
  `agenterm-cu --target current --grant observe tree --window HANDLE`.
- Permission: `AXIsProcessTrusted() == false` or `AXErrorAPIDisabled`
  → typed `a11y_permission_denied`. Wall-clock snapshot budget and
  node/depth/string limits fail typed (`a11y_tree_timeout`,
  `a11y_node_limit`, …). Never fall back to screenshot, coordinates, or
  CGEvent while reporting structured success.
- Actuation (click/focus/value) is explicitly unsupported in this cut.
- Live evidence is **not** claimed from a Linux builder. Darwin recipe:
  `scripts/cu-macos-smoke.sh` with fixture seed `345AXTREE` and button
  `Fixture Press`. A unit mock is not a live gate.

### Compare when debugging

```bash
# Running binary requirement
codesign -d -r- ~/Applications/AgentermCu.app
# TCC row (system DB; read-only under SIP)
# client com.agenterm.cu → auth_value and csreq must match the designated line above
# Host self-report after kickstart
cat ~/.local/share/agenterm/ax-status
tail ~/.local/share/agenterm/cu-hotkeys.log
```

## Unix `ioctl` needs a narrow variadic ABI path

`agenterm-dyn` `dlcall` normally uses a bounded Rust `extern "C"` fixed-arity
trampoline. Unix `ioctl(int, unsigned long, ...)` is variadic; on arm64 an
unnamed third argument is not in the same slot as a fixed third parameter.
The native door therefore recognizes only `ioctl` with
`(i32, u64|i32, ptr) -> i32`, transmutes the already-resolved `dlcall` symbol
to Rust's variadic declaration, and invokes that loaded address. Linux and macOS
smoke tests open a 24×80 pty slave and require `TIOCGWINSZ` to return the same
dimensions. All other names and signatures retain the fixed trampoline: this
is not general variadic FFI and adds no C or libffi shim.

## Six-cell `system_probes` must grow together

`agenterm-dyn` stores headless probe rows as one fixed-length
`[SystemProbe; N]` on every `{linux,macos,windows} × {x86_64,aarch64}` cell.
A Darwin-only live name still needs a same-length Placeholder on Linux and
Windows or the crate will not compile. Keep `mach_host_self` last and
Placeholder: `dlcall` has no Mach-port release owner, so a live call would
leak a send right.

Store the assembled six-cell matrix as `static`, not a copying `const`. At 82
probe rows per cell, the public `[HostCell; 6]` crossed Clippy's
`large_const_arrays` threshold; `pub static ALL_CELLS` retains one immutable
catalog allocation while preserving iteration and lookup consumers. Keep the
individual cell values `const` so target-selected references remain available.

## Do not pull `mach2` for Darwin probe baselines

`libc` 0.2 deprecates some Darwin `mach_*` types and functions toward the
`mach2` crate. `agenterm-dyn` must not take that dependency. For a probe
baseline, declare the `#[repr(C)]` layout and `unsafe extern "C"` symbol
locally (same pattern as `clock_gettime_nsec_np`) and compare `Dyn::eval`
against that later native call.

On Darwin, `pthread_t` is `usize`. `libc::pthread_threadid_np(std::ptr::null_mut(), …)`
does not type-check; pass integer `0` for the current thread. Never spell
`pthread_t` as a `dlcall` type name — it is a rejected C alias.

`os_proc_available_memory` exists in `libSystem` but the macOS SDK marks
the header unavailable (iOS-oriented). A symbol that `nm` can see is still
not a live Darwin probe if the public SDK refuses the declaration. Prefer
`dladdr`, `gethostuuid`, and `_dyld_get_image_header` for leak-free
loader/host facts.

## GNU and Darwin `strip -s` are different contracts

In GNU `strip`, `-s` strips all symbols. Apple's Darwin `strip` interprets
`-s file` as a symbol-list input, so `strip -s artifact` consumes `artifact`
as that list and then fails because no binary target remains. Portable artifact
measurement scripts must branch on the host: use `strip -x artifact` on Darwin
and `strip -s artifact` on GNU hosts. Measure the resulting artifact and run its
black-box self-test; a successful link is not size or behavior evidence.

## Guest `min` is not a host allocation size

`vec![None; table_min]` and `vec![0u8; pages * 64KiB]` take the module's
declared minimum as a trusted length. Untrusted `.wasm` can set
`table min=0x0FFFFFFF` and abort the process (SIGABRT / rc 134) before
any `Err` returns. Compare the host budget first; only then
`try_reserve`. Instantiation failure is `Err`, never "allocate and hope".
The reject point must move when the caller passes two different budgets
— a crate `const` alone is not a host contract.

The same rule applies after a byte ceiling check. `Vec::extend_from_slice`
may still invoke the infallible allocator and abort under pressure. For bytes
originating in a guest or persisted snapshot, first `try_reserve_exact` the
bounded addition and map refusal to the runtime's ordinary error path; only
then extend. A small configured limit is policy, not allocation evidence.

## Versioned media needs one discriminated SDK boundary

When one guest output channel accepts multiple versioned media schemas, do not
make the public lifecycle method unconditionally decode the first shipped
schema. Keep a single magic/version dispatcher with strict whole-record
decoders, expose the result as a typed enum, and make converter validation use
the same dispatcher. Preserve a schema-specific convenience only when existing
consumers already depend on it. Otherwise the second valid schema can pass the
WASM/runtime byte boundary and still fail every real app on its first frame.
Give a newly optional schema an ordinary version-query import so runtimes that
predate it reject the module at load; magic-only dispatch is not capability
negotiation.

For indexed pixels, bound dimensions, their checked product, palette length,
the complete stream length and every palette index before native presentation.
A host byte ceiling alone does not reject a malformed in-range index or a
pathological skinny image, while a decoder-only ceiling does not stop the
guest-to-host allocation. Both layers are required.

If several parallel integration tests need the same compiler-produced guest,
build its deterministic bytes once behind a process-wide `OnceLock` and clone
the result. Concurrently invoking one builder/output path can race rustc/linker
temporary cleanup and create a false missing-object failure even when the final
artifact path is stable.

For a whole-frame indexed guest, do not equate the wire format with repainting
every pixel in guest code on every tick. Keep the complete validated frame in
linear memory, erase/redraw only dynamic sprites during ordinary ticks, and
rebuild static pixels only at init, level reset and resume. Gate the rare
rebuild path separately under the same production fuel ceiling: measuring only
steady-state ticks can ship a deterministic step-budget trap on the first clear.

Portable state replay must compare complete render bytes, not only guest fields.
A state transition can leave an old prompt or overlay in the resident frame;
a fresh resumed instance rebuilds from logical state and exposes the mismatch.
Clear transition-owned pixels when the phase changes, then require the original
and rebuilt instances to emit byte-identical render and audio on the next tick.
