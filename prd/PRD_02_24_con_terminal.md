# `agenterm-con` terminal and rendering

Parent: [Lightweight terminal host (`agenterm-con`)](PRD_02_23_agenterm_con.md)

This module owns the standalone host's PTY ownership, VT parsing, damage,
rasterization, native present, glyph and ISA behavior. Shared kernels stay in
[terminal runtime](PRD_02_01_terminal_runtime.md) and
[native platform](PRD_02_20_native_platform.md); this module owns only what con
decides for itself.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Session ownership

- [x] each tree tab owns an independent PTY, parser, viewport and failure state
  in a single GUI process. It is explicitly not a mux, persistent workspace,
  Fleet authority or script runtime.
- [x] PTY delivery uses a platform-owned fixed 1 MiB byte ring per session
  instead of allocating a `Vec` for every native read. Each read commits
  atomically or waits for capacity; close wakes blocked producers while
  preserving committed tail bytes for draining.
- [x] parsing remains bounded to 128 KiB per GUI turn. Reader wakes are
  coalesced, inactive tabs are drained without forcing unrelated active-tab
  paints, and remaining backlog yields to input before self-scheduling another
  turn.
- [x] the Windows production graph no longer contains `rmux-pty`. The direct
  adapter owns synchronous ConPTY endpoints, a cancellable overlapped writer, a
  drain-safe output pump, build-gated passthrough fallback, PowerShell DSR
  fragments, suspended process creation, exact wait/exit status, and a
  `KILL_ON_JOB_CLOSE` Job Object. It never resumes an unassigned child and never
  reuses a ConPTY whose failed first child may have closed its output pump.
- [x] the primary thread handle from suspended `CreateProcessW` is owned only
  until `ResumeThread` and PID validation succeed, then closed immediately
  through `OwnedHandle` Drop. Failure before successful resume remains armed to
  terminate the partial child; Job assignment-before-resume and independent
  process, Job and HPCON ownership are unchanged.
- [x] Windows process termination keeps its native wait leaf allocation-free:
  the 500 ms `WaitForSingleObject` result and immediately captured
  `GetLastError` become `Exited`, `Running`, or `Failed(code)`, and only the
  explicit terminate caller constructs a typed error.
- [x] the child waiter publishes one completion bit directly rather than
  instantiating a production channel for a `()` value. Release stores the state
  before the existing window wake; the GUI consumes it once with an
  acquire-release swap, keeping process-exit authority independent from ConPTY
  pipe EOF.
- [x] Windows ConPTY environment inheritance calls `GetEnvironmentStringsW`
  directly and frees that borrowed block through one adapter-owned RAII guard. A
  bounded UTF-16 scan and ordered streaming merge preserve hidden `=C:` drive
  entries, inherited variables, ASCII-case-insensitive last-override semantics,
  validation and the required double NUL before `CreateProcessW`.
- [x] Windows environment-block construction uses a PTY-private sorted vector
  with explicit binary insertion instead of the generic BTree node allocator,
  preserving normalized UTF-16 key order, case-insensitive last-write
  replacement, original override spelling and the exact double-NUL block.
- [x] Windows executable admission compares the exact `.exe`/`.com` PATHEXT leaf
  directly over native UTF-16 code units, including non-Unicode rejection.
  PATHEXT enumeration stays native end to end through a bounded four-unit
  streaming classifier that preserves extensionless-first lookup, configured
  order, duplicates, empty-list fallback and invalid-nonempty suppression while
  emitting only canonical `.EXE`/`.COM` candidates.
- [x] PTY teardown closes bounded output backpressure on the event thread, then
  transfers master/child ownership to platform-managed background teardown. That
  owner terminates the child tree, closes the pseudoconsole and releases both
  halves through one bounded platform reaper rather than creating a native
  thread per closed tab, without stalling UI or control progress. Reaper
  readiness is a prerequisite to native PTY creation, so thread-start failure
  rejects the new terminal before any potentially blocking native ownership
  exists. A black-box journey runs four concurrent million-line producers,
  closes every flooded tab within a strict per-tab deadline, verifies stable IDs
  leave the tree, and proves an unaffected sibling remains interactive after
  every close. The generic bounded-close rule for a tab and its workers stays
  with [terminal runtime](PRD_02_01_terminal_runtime.md).
- [x] a public multi-tab journey uses `wait-tab-exit` to prove one child can
  exit with a non-zero code while a sibling continues accepting input, the dead
  tab remains capturable, parent promotion still holds, and closing the final
  retained tab ends the GUI cleanly.

## Scheduling

- [x] a native Wake services the bounded control queue first and shares one
  fixed PTY byte budget across all live tabs. Background output therefore cannot
  multiply one event's work by tab count or starve the command that stops it.

## Input delivery to the child

- [x] Windows native key delivery uses one platform `ConsoleGuard` to serialize
  `FreeConsole`/`AttachConsole`, retries the documented already-attached case,
  opens `CONIN$`, and writes an exact key-down/key-up `INPUT_RECORD` pair with
  `WriteConsoleInputW`. The real `cmd.exe` cursor journey and the
  alternate-screen `less` arrow/wheel journey are default black-box tests rather
  than ignored known gaps.
- [x] named terminal-key aliases are platform-owned rather than duplicated in
  con and the workbench. The shared parser is allocation-free and
  ASCII-case-insensitive; con rejects an unknown multi-character key while
  workbench UI injection still treats unsupported names as literal text.

## Damage, raster and present

- [x] vendored `vt100` emits allocation-free conservative row damage from
  mutation sites. Exact visible-cell comparison is its collision-free test
  oracle, while unknown callbacks, viewport changes, resize and alternate-screen
  transitions fail safely to full. PTY Wake drains this evidence before
  invalidation, so ordinary output requests only the affected terminal rows and
  the old/new cursor overlays instead of invalidating the complete client.
- [x] the pixel-window contract identifies retained versus transient host
  backing and requires each frame to commit `None`, `Full`, or a bounded partial
  rectangle. Windows rasterizes directly into the retained native XRGB buffer,
  forces full raster after allocation/resize/DPI invalidation, and has removed
  the former product-to-host full-frame copy; Unix/macOS retain the
  product-owned bounded frame and full-copy it into explicitly transient
  softbuffer frames.
- [x] Windows maps typed physical damage to `InvalidateRect` and uses
  `PAINTSTRUCT.rcPaint` for top-down `StretchDIBits` partial present. Every
  successful `BeginPaint` is paired with exactly one `EndPaint`, short scanline
  copies are rejected, and a renderer error is never presented. Unix catches
  application callback panics at the event-loop boundary and converts them to
  typed failure.
- [x] Windows retained pixel windows deliberately omit `CS_HREDRAW` and
  `CS_VREDRAW`; system expose, typed invalidation and settled geometry redraw
  are the only paint authorities.
- [x] the native ledger times `StretchDIBits` or softbuffer `present` itself
  without a GUI-thread lock, and reports OS expose pixels separately from
  product damage rather than relabeling expose as damage.
- [x] Win32 maps `WM_ENTERSIZEMOVE`/`WM_EXITSIZEMOVE` to shared optional
  interaction events. During that lifetime current metrics track the client
  while GDI scales the last complete retained DIB under the native paint clip;
  product generation, VT/PTY geometry and raster settle once on exit.
  Unix/macOS retain ordinary geometry plus trailing-edge debounce fallback.
- [x] when capture temporarily renders an inactive target, the platform receives
  an explicit discarded-frame receipt: neither retained nor transient hosts may
  present that scratch frame before the restored active tab redraws. Public
  performance evidence counts discarded capture frames, so the select/capture
  black-box journey proves the running host used that path exactly once.
- [x] the Win32 native host keeps `GWLP_USERDATA` behind a reentrancy-checked
  owner, defers callback-issued synchronous User32/IMM commands until
  application and framebuffer borrows end, snapshots stateful reentrant messages
  without retaining pointer-backed parameters, validates and reschedules nested
  paint, and fails closed on bounded-queue overflow or nonconvergence.

## Fonts and glyphs

- [x] Windows glyph selection and gray8 coverage execute behind the platform
  `RasterGlyph` contract through bounded GDI calls with deterministic DC/font
  cleanup. Con no longer opens or parses font files; ab_glyph and ttf_parser are
  absent from its Windows production graph. Linux/macOS retain equivalent
  file-font behavior inside a shared platform portable adapter.
- [x] the Windows GDI leaf maps supplementary scalars through the selected
  face's bounded OpenType `cmap` format-12 table, then rasterizes the resulting
  glyph index through the existing bounded `GetGlyphOutlineW` path. BMP lookup
  remains on `GetGlyphIndicesW`; malformed, oversized or absent tables fail as
  a local missing glyph without splitting surrogate pairs. Color emoji,
  variation selectors and run shaping remain an explicit later DirectWrite
  tradeoff rather than a shipped claim.
- [x] Windows native glyph faces survive platform raster cache misses at one
  active pixel size: the adapter lazily creates only the GDI families reached by
  coverage, keeps each HDC/HFONT on its creating thread, and drops the whole RAII
  set when zoom selects another size, following the documented
  `CreateCompatibleDC(NULL)` thread-ownership rule rather than asserting an
  unsafe cross-thread `Send`. A deterministic 94-printable-ASCII probe reduced
  native face creation from 94 sequences to one.
- [x] shared glyph caching stores its read-heavy bounded FIFO values in a sorted
  contiguous vector, preserving deterministic eviction without retaining
  randomized hashing.

## Architecture kernels

- [x] x86_64 ISA dispatch has one shared authority rather than independent
  standard-library and dependency detectors. Vendored vt100 disables VTE's
  std-only memchr runtime dispatch (VTE parser semantics unchanged, ESC scans
  retain mandatory x86_64 SSE2); UI-core caches pixel blend and RGB pack
  function pointers from one CPUID probe, using bounded `xgetbv` inline assembly
  before any AVX2 call to require OS-managed XMM/YMM state. The probe agrees
  with the standard oracle in tests.
- [x] shared x86 pixel dispatch gives blend and RGB8 pack independent lazy ISA
  selectors, so the standalone host — which consumes only blend — drops the
  unused SSSE3 pack kernel, scalar pack kernel and SSSE3 selector to zero bytes
  in final-link evidence while retaining AVX2/SSE2 blend.
- [x] native geometry does not delegate basic IEEE-754 rounding to CRT math
  imports: a shared product-neutral platform leaf implements exact bit-level
  `round_f32`, `round_f64`, `ceil_f32` and `trunc_f32`, consumed by con UI,
  font, wheel and the Windows pixel host, with standard-library oracle tests over
  boundary values and sampled bit patterns.

## Rendering performance evidence

Frame, damage and present counters are exposed through `perf-stats` /
`reset-perf-stats` ([26](PRD_02_26_con_control_cli.md)): conservative
full/partial raster-candidate frames, dirty and frame pixels, platform-owned
native-present count, latency, requested/completed pixels, and host direct/copy
frame and pixel counts.

- [x] the 16-step public resize journey dropped from 35 to 18 frames, 17 to 8
  full candidates, 9,632,800 to 6,485,040 dirty pixels, and 25.715 ms to
  16.302 ms native present time with unchanged PNG geometry, zero present
  failures and zero host copies. After the size-move interaction contract the
  same journey needs only 3-5 product frames and 1-2 full candidates, with 11-13
  successful platform presents, no failures or copies, and at least a 10x
  dirty-pixel reduction in the best observed run.
- [x] a paired Windows release probe measured seven idle partial frames at
  895 us average before direct backing versus eight at 360 us after it
  (59.8% lower); a 50-step send/wait journey measured 244 frames at 1,310 us
  average versus 250 at 992 us (24.3% lower), with all 250 new frames direct and
  zero copied frames or pixels.
- [x] a post-row-damage Windows release probe produced 33/33 partial raster
  candidates with 70,560 dirty pixels over 17,476,272 frame pixels (about
  0.40%), 33/33 successful native presents, a 1,589,501-byte PNG and captured
  command text before explicit `@1` close. The native ledger separately reported
  529,584 full-expose pixels and 70,560 partial pixels.
- [x] a public 100-title OSC stress completed 883/883 direct frames with zero
  host copies and zero present failures while producing a valid 1,589,501-byte
  screenshot.
- Historical only: pre-row-damage probes observed 2/5 partial candidates at
  about 60.0% dirty pixels for blink/idle, falling to 2/13 and about 84.6% after
  mixed PTY output. These are directional evidence, not release qualification.
- [x] con's dedicated public sustained-output qualification delivers a fixed
  31.9 MiB payload at no less than 2 MiB/s with a 30-second hard deadline while
  a sibling tab responds within five seconds and repeated `list-tabs` /
  `perf-stats` observations stay below two seconds. Its receipt requires the
  complete PTY byte count, at least one bounded-budget yield, zero native-present
  failures, zero host copies and clean `close-window` shutdown. This is distinct
  from the OSC and close-under-flood robustness journeys.
