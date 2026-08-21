## Design Document Review: rh: standalone dynamic language product

### Summary
Needs revision. The product direction (interpreter-default, `rh.com` as loader-only, in-tree extractability before `partnernetsoftware/rh`, freeze list that keeps rustc/Cranelift/`rh_entry` out of the embedder API) is sound and mostly matches the live tree, but an engineer cannot implement the embed API, Language 1, or the B2 pin from this text without resolving several blocking gaps: `rhai::AST` is `!Send` versus D17, Language 1 is defined as two incompatible surfaces, and a public AgenTerm pin of a private rh repo would break `cargo build`.

### Issue 1: `CheckedAst` wrapping `rhai::AST` cannot satisfy D17 (`Engine: Send`) and freezes the wrong IR for AOT/JIT
- **Severity**: critical
- **Section**: Key Decisions D7/D10/D17; Proposed Design §6; API / Interface Changes (Embedder API, Backend trait)
- **Description**: D17 freezes `Engine` as `Send` and not `Sync`. §6 says the parser keeps `rhai::Engine::compile` and wraps the result in `CheckedAst`. The Backend seam is `fn eval(&mut self, ast: &CheckedAst, ...)`. docs.rs for `rhai` 1.22.0 (`struct.AST`) auto-traits are `!Send` and `!Sync` (same on 1.25). A `CheckedAst` that owns `rhai::AST` makes `Engine` and `Compiled` `!Send`, so D17 is unsatisfiable. It also couples every future `Backend` (interp, Cranelift, rustc pack) to rhai internals, which contradicts D7 (“replacing the parser later must not break embedders”) and D10 (“do not freeze codegen IR”). Walking rhai nodes in-tree is an implementation tactic, not a freeze surface.
- **Suggestion**: Lower at `check` time into a crate-owned, `Send` IR (the real Language-1 AST). Keep `rhai::*` crate-private. Make `Backend` crate-private for v1, *or* have it consume the owned IR rather than `CheckedAst` as a rhai wrapper. `Engine::compile` / `Compiled` then store that IR or an opaque blob. Add an explicit non-goal: “do not expose or `Send` `rhai::AST`.” PR-A1/A2 must include the lowering, not “share `CheckedAst`” as today’s `subset.rs` `rhai::AST`.
- **Status**: addressed
- **Response**: Agreed. Dropped `CheckedAst` as a rhai wrapper. Check lowers to a crate-private `Send` `IrModule`, then drops the `rhai::AST`. `Backend` is `pub(crate)` and takes `&IrModule`. `Engine`/`Compiled` store IR, not rhai types. Non-goal added. PR-A1 includes the lowering plus a `Engine: Send` thread-spawn test.

### Issue 2: PR-B2 git-pin of a *private* `partnernetsoftware/rh` from public AgenTerm breaks the wbox pattern
- **Severity**: critical
- **Section**: Key Decisions D1/D2; Rollout Plan; PR Plan PR-B2
- **Description**: D2 copies the wbox pattern: `wbox/Cargo.toml` has `agenterm-platform = { git = "https://github.com/mgttt/agenterm.git", rev = "c8ace42", default-features = false }` — a **public** repo (verified). AgenTerm itself is public (`Cargo.toml` `repository = "https://github.com/partnernetsoftware/agenterm"`). PR-B2 replaces `crates/agenterm-rh` with `rh-lang = { git = "https://github.com/partnernetsoftware/rh.git", rev = "<fullsha>" }` while the rh repo is still **private**. Anyone who clones public AgenTerm then cannot `cargo build` / `cargo test` without GitHub credentials for a repo that does not exist yet (GitHub search `org:partnernetsoftware rh` returns `total_count: 0`). That is not the wbox pattern. Open Question 5 only asks about collaborators / whether wbox should pin rh, not about public CI/clone.
- **Suggestion**: Until PR-E1 (public + crates.io), keep a path workspace member in AgenTerm (`crates/agenterm-rh` as adapter or vendored `rh-lang` sources). Use the private GitHub repo for the product CLI/releases only. Git-pin from AgenTerm only when the rh repo is world-readable, or document that AgenTerm itself goes private (not proposed). Do not treat “wbox pins public agenterm-platform” as evidence that public agenterm can pin private rh.
- **Status**: addressed
- **Response**: Agreed. D2 now states AgenTerm keeps a path member until rh is public; wbox cite is the actual `mgttt/agenterm` URL. PR-B2 is “no git pin.” PR-E1 order is public → SHA pin → crates.io. Private repo is CLI/releases only.

### Issue 3: Language 1 is defined as two different languages
- **Severity**: critical
- **Section**: Goals 3; Key Decisions D8/D15/D19/D20; Proposed Design §6; API Host trait; PR-A2/A3/A6
- **Description**: §6 says **Language 1 ≡** “the subset `agenterm_rh::check` accepts **and** native transpile (`CdylibExecutionMode::Native`) can emit.” Those are not the same set, and neither matches the StdHost/Value sketch.

  1. `check()` in `crates/agenterm-rh/src/check.rs` is *not* the rh-3 subset. On `validate_ast` failure it calls `compat_validate`, which only rejects `eval`. Closures / `do` / `switch` / interpolation therefore `check` ok if they parse. PR-A6 “route check through `Engine::check` (compat), no behavior change” would bake that bypass into the product checker.

  2. Native transpile’s `ValueKind` (`transpile.rs`) is `Int | Bool | String | Char | Json | Set | StringList | Metadata | SystemTime | DirEntry | Path | Command | Output | Child | ChildList | WindowControl | WindowRect | Stream | Bytes | Task | FileLock`. D20 freezes product `Value` as `Unit | Bool | Int | String | Array | Map | Bytes | HostObject` with Window/Child/Task/Fleet as host objects. D19 puts process in StdHost but the Host trait only has `process_status(&ProcessRequest) -> i32`. Live `std` is `Command.start` / `Child.wait` / `Output` / `DirEntry` / `PathBuf` (see `shipped_surfaces.rs` and fixtures `command-arg-probe.rh`, `child-stdout-probe.rh`, `process-kill-probe.rh`). PR-A3 then asks to match those probes “without rustc.”

  3. Semantic lock: “a fixture that `check`s and native-transpiles must eval identically on the interpreter for the Language-1 core.” Core is then parenthetically narrowed to ints/strings/maps/`std::fs`/`env`/`process`, which is a *third* definition. An implementer cannot know whether `Command`/`Child`/`Path`/`Json` vs `Array`/`Map`, `require`/project imports (`project_import.rs` stays in AgenTerm), `rh::json`/`bytes`/`hash`/`crypto`, or `type_of`/`debug` are Language 1.

- **Suggestion**: Publish a closed Language-1 spec: syntax (what `validate_ast` rejects, **without** `compat_validate`), value model (how `Json`/`Path`/`Char`/`Set` map to `Value`), and a **name allowlist** of `std::`/`rh::`/`print`/`args` surfaces with types. State explicitly that `agenterm_rh::check`’s compat bypass is AgenTerm-only. `Engine::check` = strict subset + trimmed allowlist. Native-only types (`Child`, GUI, `Task`, TCP, HTTP) are either HostObject + `Host::call` in AgenTerm, or out of Language 1. Do not cite `command-arg-probe.rh` as a StdHost golden unless Command objects are in v1.
- **Status**: addressed
- **Response**: Added a closed **Language 1** section. `Engine::check` = `validate_ast` without `compat_validate` + allowlist. Compat bypass is AgenTerm-only. ValueKind mapping table: Json→Array/Map/scalars, Char→String, Set rejected, Path/Command/Child/… → HostObject type_ids; Window/Task/TCP/HTTP/Fleet out. Command/Child **are** Language 1; `command-arg-probe.rh` and siblings are StdHost goldens. PR-A6 no longer silently strict-ifies `agenterm rh check`.

### Issue 4: Host trait as sketched cannot implement D13 or D19, and is not a freeze-ready surface
- **Severity**: major
- **Section**: Key Decisions D9/D10/D13/D19; API / Interface Changes (`Host` trait); PR-A3; AgenTerm as embedder
- **Description**: D13 says sandbox embedders “omit those methods (they fail closed).” The trait lists `print`, `args_len`, `arg`, `fs_exists`, `fs_read_to_string`, `fs_write`, `process_status` as **required** methods; only `call` has a default. In Rust you cannot omit them. Lua’s precedent (`LuaHostFunctions` in `crates/agenterm-lua/src/lib.rs`) is `Option<Arc<dyn Fn…>>`, which actually allows omit.

  D19 / PR-A3 put json/bytes/hash/crypto-sha256/env/path/time/process-stdout_file **in** the default host. None of those are trait methods. If they go through `Host::call`, the **name strings** are the freeze surface and are unspecified (`std::fs::exists` vs `fs_exists` vs `std.fs.exists`; `fleet.tabs.list` vs `fleet::tabs::list`). D10 claims to freeze “Host trait + default-host module set” but neither the method set nor the module set is listed.

  `ProcessRequest` is unnamed. Live caps in `src/script_rh_host.rs` are 256 args × 4096 bytes, 256 env entries, 16 MiB reads (`RH_HOST_FS_READ_CAP`) — the security table copies these, the trait does not.

- **Suggestion**: Either (a) give every capability a default `Err(Error::unsupported)` method, or (b) keep a small required trait (`print`/`call`) plus `StdHost` methods. Publish the v1 name allowlist and JSON/bytes/path encodings. Specify `Host::call` naming for AgenTerm `fleet.*` so PR-D1 is implementable. Put caps on `StdHost`/`Options`, not as folklore.
- **Status**: addressed
- **Response**: Chose defaulted trait methods (`print`/`args_len`/`arg`/`call` all default `unsupported`) so omit is real. Language-1 surfaces go through `Host::call` with **script spelling** (`std::fs::exists`, `Command.start`). AgenTerm fleet uses **dot** form (`fleet.tabs.list`). `ProcessRequest` and caps are on `StdHost`/`Options` with the live numbers from `host_process_request` / `RH_HOST_FS_READ_CAP`.

### Issue 5: Moving `subset.rs` / `check.rs` / `expr_print.rs` as-is pulls Fleet into `rh-lang`
- **Severity**: major
- **Section**: Proposed Design §8; PR-A2; PR-B1
- **Description**: The design says Fleet stays in AgenTerm and rh-the-product must not own it. Live coupling is the opposite: `subset.rs` `use crate::fleet::{expr_uses_fleet, parse_fleet_call, validate_fleet_call}` and emits `RH_SUBSET_FLEET_SHAPE`; `expr_print.rs` `uses_host_surface` treats identifiers `std | rh | rhai | fleet` as host; `api_validate.rs` validates `SHIPPED_SURFACE_PATHS` including 76 `fleet.*` entries; `check_with_project_validation` runs that allowlist. §8 “Move subset.rs, check.rs, expr_print.rs” without a split copies Fleet grammar into the product crate. Embed API then has hidden coupling to AgenTerm’s fleet facade even when `Host` is `StdHost`.
- **Suggestion**: Split before PR-A2: language subset (loops, closures, interpolation, assign, while-int, try/catch, no `eval`) vs fleet-shape validation vs shipped-API allowlist. Product `Engine::check` uses language subset + non-fleet allowlist. AgenTerm adapter keeps fleet-shape + full `shipped_surfaces`. Do not move `fleet.rs` even as a dependency of `subset.rs`.
- **Status**: addressed
- **Response**: Split is PR-A1 (before interp). Product `Engine::check` does not run `RH_SUBSET_FLEET_SHAPE`. `fleet.rs` stays in AgenTerm. §8 move table rewritten.

### Issue 6: Recommended `rh-slice-abi/1` trailer (EOF−64) fights D14 codesigning
- **Severity**: major
- **Section**: Key Decisions D10/D14; Proposed Design §7; Data Model (slice header); PR-C1
- **Description**: D10 **freezes** the slice header. §7 recommends a 64-byte blob at **file end minus 64** with `RHsl` magic, and says “pick one in implementation PR.” D14 forbids first-run rewrite and requires notarizing Mach-O loader + slices. Mach-O `LC_CODE_SIGNATURE` and PE Authenticode both live at EOF; a trailer is either unsigned (Gatekeeper) or overwritten by the signature. The freeze-vs-“pick later” split is the problem: if C1 ships EOF−64, a later named-section fix is an `rh-slice-abi` break. `payload_sha256` “first 8 bytes of hash, or 0” is circular if the header sits inside the hashed file, and packed product semver in a frozen ABI couples crate numbers to loader checks for no stated reason.
- **Suggestion**: Do not freeze placement until it survives codesign. Recommend a named section (`__DATA,__rhslice` / `.rhslice` / PE resource) **inside** the image, then sign. Keep `abi_version` + cell id as the fail-closed fields; drop or ignore product semver; define hash as “image bytes excluding the header” or drop it in v1 (thin layout already trusts same-dir). Loader still fail-closes unknown `abi_version` and cell mismatch.
- **Status**: addressed
- **Response**: Frozen placement: ELF `.rhslice`, Mach-O `__DATA,__rhslice`, PE `.rhslice`, **then** sign. 32-byte payload: magic, `abi_version`, cell, flags, reserved-0. Dropped semver and hash. EOF trailer explicitly rejected. No “pick in C1.”

### Issue 7: PR sequence drops the feature-split before repo creation and overstates the A4 gate
- **Severity**: major
- **Section**: PR Plan PR-A4, PR-A5, PR-B1; Rollout mermaid
- **Description**: “No GitHub repo until PR-A4 is green” is the right lock, but A4’s stated gate is weak and B1 does not depend on A5.

  - `crates/agenterm-rh` already does **not** depend on the `agenterm` package (`Cargo.toml` deps: `agenterm-script-common`, `rhai`, `sha2`, `serde`, `tempfile`, `libloading`). `cargo tree -p agenterm-rh` not listing `agenterm` is **already true**. The real missing piece is a runnable host, which A3+A4 add. `nm` on a cdylib/rlib also will not show `agenterm` symbols today.

  - B1 “create partnernetsoftware/rh from extractable agenterm-rh” depends only on A4, not A5. A5 is what drops `libloading`/`tempfile` from the default graph (`compile` feature). Copying the crate at A4 still ships rustc-pack deps in `rh-lang` default, contradicting D8/D12 and “default embedder graph: no libloading.”

  - After A5, `cargo test -p agenterm-rh` (no feature unification with the root package) will fail crate tests that call `compile_native` / pack (`tests/fixture_probe_native.rs`, qualify paths) unless those tests are `cfg(feature = "compile")`. A5 does not mention gating tests. Workspace `cargo test` will still unify `compile` once root enables `agenterm-rh/compile`.

- **Suggestion**: Redefine A4 as: `cargo run -p agenterm-rh --example standalone_eval -- <file.rh>` executes print/fs with **zero** root-package objects in the link, plus a fixture that fails if `StdHost` is missing. Make B1 depend on **A4 and A5**, or do the `compile` feature split inside B1. Gate pack tests on `feature = "compile"`. Keep `cargo tree -p agenterm-rh` as a regression check, not the extractability proof.
- **Status**: addressed
- **Response**: A4 is a runnable StdHost example; `cargo tree` is a regression check (already true). B1 depends on A4 **and** A5. A5 `cfg`-gates pack tests. Background table states the crate never depended on the `agenterm` package.

### Issue 8: Windows loader handoff is not an `execve`
- **Severity**: major
- **Section**: Proposed Design §7 step 4; runtime sequence diagram
- **Description**: Unix `execve` replaces the loader process, so the slice’s exit code is the user’s exit code and argv/env/cwd are naturally preserved. Windows `CreateProcessW` **spawns a child**. The text never says the loader waits (`WaitForSingleObject`) and forwards `GetExitCodeProcess`. Without that, `rh.com script.rh` returns 0 immediately and Ctrl-C/job control attach to the wrong process. `lpCommandLine` reconstruction (quoting) is also unspecified. argv[0] becomes the slice path — fine, but rh-cli must still find the script path in remaining argv after a loader hop (`rh.com` vs `rh-osx64arm.com`).
- **Suggestion**: Specify: Windows loader is a parent that waits and copies the exit code; Unix uses `execve`. Document Ctrl-C / console control group. Add a quoting algorithm or use `argv[]` via `CreateProcessW` with an explicit wide argv helper. Test: `rh.com -e "fn entry(){ 3 }"` exits 3 on Windows too.
- **Status**: addressed
- **Response**: D23 + §7: Unix `execve`; Windows parent `CreateProcessW` (libstd quoting), share console, parent ignores Ctrl-C while waiting, `WaitForSingleObject` + `GetExitCodeProcess` + `ExitProcess`. Sequence diagram split. PR-C1 includes the exit-3 test. rh-cli finds the script as the first remaining positional after argv[0]=slice.

### Issue 9: Backend trait publicity vs reserved AOT/JIT
- **Severity**: major
- **Section**: D10; Backend trait; “What would break if we add Cranelift”
- **Description**: The freeze table correctly does **not** freeze rustc pack format, `rh_entry(): i64`, `RH_CODEGEN_REVISION`, Cranelift, or `libloading`. The public `Engine::compile` → `Unsupported` default is the right embedder seam. What is wrong is publishing `trait Backend { fn eval(..., ast: &CheckedAst, ...) }` without saying whether embedders may implement it. If `pub`, `CheckedAst` and `Limits` become frozen (and today would be rhai). If crate-private, Cranelift can be added in-tree without an embedder break, which is what D10 wants. `RustcPackBackend` listed under `rh-lang` `feature = "compile"` also fights §8 “transpile/compile/pack/load/host_api C ABI stay in AgenTerm initially.” Putting rustc pack into the product crate even behind a feature reintroduces `libloading` into the rh repo and invites embedders to enable it.
- **Suggestion**: v1: `Backend` crate-private; public freeze is `Engine::{new, eval, check, compile}` + `Host` + `Value`. Keep rustc pack in AgenTerm’s adapter (`crates/agenterm-rh` post-B2) until a later, explicit product decision. `Engine::compile` may stay as a reserved `Unsupported` in `rh-lang` without a `RustcPackBackend` type in that crate.
- **Status**: addressed
- **Response**: `Backend` is crate-private. No `RustcPackBackend` in `rh-lang`. No `compile` feature on the product crate in v1. rustc pack stays in the AgenTerm adapter. Public freeze is Engine/Host/Value/`compile`→Unsupported. Alternative A8 records the rejected public-Backend idea.

### Issue 10: CLI shebang fuel default is the untrusted-worker budget
- **Severity**: minor
- **Section**: API `Options`; Observability/Security; Open Question 3; Risks
- **Description**: `ScriptBudgets::default()` (`src/script_protocol.rs`) is `operations: 1_000_000`, `wall_time_ms: 2_000` — for workbench workers. The design copies fuel 1e6 onto **CLI shebang** (“scripts as tools”) while turning wall-time off. A Node-like `#!/usr/bin/env rh` that loops over files will `OutOfFuel` far sooner than users expect. Open Question 3 only asks about wall-time, not fuel. Embedder 2s default is appropriate; shebang 1e6 ops is not argued.
- **Suggestion**: Shebang: fuel **off** (or a much higher cap) matching “user shell tool”; embedder/AgenTerm: keep 1e6 + 2s. Expose `--fuel` / `--timeout-ms`. Confirm as an owner question alongside Q3.
- **Status**: addressed
- **Response**: Decided, not left as a question. D22: CLI fuel **off**, wall-time **off**, `--fuel` / `--timeout-ms` opt-in. Embedder `Engine::new()` keeps 1e6 + 2s. Parse depth 512 vs runtime 64 cited from `RH_MAX_EXPR_DEPTH` / `ScriptBudgets.expression_depth`.

### Issue 11: PR-C3 polyglot loader is optional given C2, but the mermaid makes D wait on C
- **Severity**: minor
- **Section**: Rollout mermaid; PR-C2/C3; PR-D1 depends
- **Description**: C2 already ships a per-cell `rh.com` next to the slice (two files per zip). That unblocks shebang without Thompson-shell/MZ polyglot. C3 then takes on zsh<5.9, WINE-detects-MZ, WSL interop, Gatekeeper — exactly the §6.3 “hostile launcher surface” the cited research warns about. PR-D1 (`agenterm rh eval` → interp) correctly depends only on B3+A3, not C, but the mermaid is `A → B → C → D → E`, which will be read as a gate. Polyglot is also how APE **must** self-rewrite to own offset 0 (`reference-cross-target-execution.md` §6.3); C3 says no self-rewrite, so Linux/Windows “polyglot rh.com” still cannot be a real Mach-O *and* PE *and* ELF without a fourth format or a shell prefix. That design tension is not resolved.
- **Suggestion**: Treat C2 as the v1 delivery; move C3 to “Later / not scheduled” with fat overlay unless an owner explicitly wants `rh.com` on Unix PATH as a polyglot. Keep D1 independent of C in both the mermaid and the prose.
- **Status**: addressed
- **Response**: C2 is v1. Former C3 moved to Later (with the §6.3 rewrite contradiction named). Rollout mermaid: D does not wait on C. D1 depends on B3+A3 only.

### Issue 12: `Engine::eval` / `eval_file` vs required `entry()` is still dual-defined
- **Severity**: minor
- **Section**: Proposed Design §6; CLI; Open Question 4
- **Description**: §6 says require `entry()` for `rh.com file.rh` “matching AOT packs; `eval` of a snippet may omit it. Confirm in CLI section.” CLI then requires `entry()` for files and allows `-e` as a single expression. Open Question 4 asks the same thing again. `Engine::eval_file` in the sketch returns `Value` with no `entry()` rule. AOT packs today are `rh_entry(): i64` (`load.rs` `call_entry`). Embedders vs CLI vs AOT can drift (last expression vs `entry()` vs truncated `u8`). Live `script_exit_code` takes `i32` and `u8::try_from`, not “i64 truncated to u8” in all paths.
- **Suggestion**: Delete the duplicate question. Freeze: files run by CLI require `fn entry()`; `Engine::eval` / `-e` use last expression or `entry()` if present; process exit is `i64 as u8` with the same wrap/saturate behavior as `script_exit_code`. Document `eval_file` the same as CLI file run or give it an `Options` flag.
- **Status**: addressed
- **Response**: D21 + CLI: files and `eval_file` require `fn entry()` (`Options.entry_required` default true); `eval`/`-e` use `entry()` if present else last expression. `exit_from_int`: `i32::try_from(i64)` then `u8::try_from`, else 1 — same as `script_exit_code` (`FAILURE`=1), with an explicit i64→i32 step because `rh_entry` is i64. Open Question 4 removed.

### Issue 13: Extractability still inherits `agenterm-script-common` until B1 strips it
- **Severity**: minor
- **Section**: Background table; Non-goals; §5 features; PR-A4
- **Description**: Correct that lua/qjs must not git-pin rh and that script-common stays. In-tree `agenterm-rh` **does** depend on `agenterm-script-common` (`check_many.rs` re-exports its driver). A4 “standalone example” can still link script-common (walkdir, check-many types) and still satisfy “no agenterm package.” That is not a product-embedder graph (`rhai + serde_json + sha2` only). Harmless for A4 if B1 strips it; dangerous if B1 is a verbatim crate copy.
- **Suggestion**: A4 example may keep the in-tree dep. B1 must not copy `check_many.rs` / `corpus.rs` / script-common. State that in B1’s file list (already implied by “not Fleet modules”; make script-common explicit).
- **Status**: addressed
- **Response**: A4 may still link script-common. B1 file list explicitly forbids copying `check_many.rs`, `corpus.rs`, and depending on `agenterm-script-common`. Default product graph is rhai+serde_json+sha2.

### Issue 14: Observability metrics are specified without an export path
- **Severity**: minor
- **Section**: Observability
- **Description**: `rh_eval_total{result=...}` / duration histogram / fuel gauge are listed “slice / embedder (AgenTerm can wrap).” Product `rh.com` is a local CLI with “no phone-home” and no metrics runtime. Unclear whether v1 must link a metrics crate (conflicts with slice size `< 4 MiB` and loader TCB) or just reserve names.
- **Suggestion**: v1: structured one-line stderr (already: `rh parse error:`, `rh subset [CODE]:`) + process exit. Defer Prometheus names to AgenTerm’s wrapper. Keep the loader reject reasons as exit 2 + stderr token.
- **Status**: addressed
- **Response**: v1 is stderr + exit only. No metrics crate. Prometheus names reserved for AgenTerm’s wrapper. Non-goal added.

### Issue 15: Small factual / consistency nits
- **Severity**: nit
- **Section**: Background; CLI sketch; D11; wbox cite; Pain point 5
- **Description**:
  - crates.io verified 2026-08-21: `rh` **exists** 0.1.14, ~7918 downloads; `rh-lang`, `rhlang`, `rh-engine` 404. D11 is correct.
  - GitHub `org:partnernetsoftware rh` search: empty. Correct that the repo does not exist.
  - wbox pin is `https://github.com/mgttt/agenterm.git` rev `c8ace42`, **not** `partnernetsoftware/agenterm`. Pattern (git + full SHA + `default-features = false`) still holds.
  - `tests/rh_regression.rs` and `tests/rh_codegen_fixtures.rs` live in the **root** package (`RH_HOST_API_VERSION == 13`, `RH_CODEGEN_REVISION == 107`); crate tests are `crates/agenterm-rh/tests/public_contract.rs` / `codegen_native_pack_fixtures.rs`. The background table is acceptable if read as repo-relative, but “pinned in tests/rh_regression.rs” is easy to miss.
  - Pain point 5 / PRD_02_10: 76 `fleet.*` paths in `shipped_surfaces.rs` is correct; PRD says 32 lack `OPERATION_CATALOG`. Live count of `script_surface: "fleet…"` in `src/operations.rs` is 43, so 76−43 = **33** missing, not 32. Copy-paste from a slightly stale PRD pin.
  - CLI sketch still has `rh.com eval` “prints `rh eval ok` envelope?” then later correctly decides no. Drop the `?`.
  - `script_engine.rs` module docs still claim root `[[bin]] name = "agenterm-rh" path = crates/agenterm-rh/src/main.rs`; root `Cargo.toml` bins are only `agenterm`, `agenterm-com`, `agenterm-cc`. The design is right; that comment is stale (not the design’s bug, but D15 “do not resurrect agenterm-rh.exe” should not assume the bin target still exists).
  - Chassis `CELLS` / `native_cell()` match the D4 table; `"unknown"` is the `_` arm. Loader fail-closed idea matches `agenterm-chassis-loader` `check_native_cell` (unknown **or** mismatch rejects). Duplicating six strings is justified (A5).
  - `RH_MAX_EXPR_DEPTH = 512` and `OptimizationLevel::None` match `check.rs`. `ScriptBudgets.expression_depth` default is 64 — the dual 512-parse / 64-runtime in `Options` is real, not a typo, but should cite both constants.
- **Suggestion**: Fix the 32 vs 33 figure or cite the parity allowlist test; drop the eval-envelope question mark; note wbox’s actual git URL.
- **Status**: addressed
- **Response**: Background table now distinguishes root vs crate tests, cites `mgttt/agenterm`, notes the retired bin target, records 76−43=**33** and the parity allowlist (comment still says 32; array has 33). CLI has no envelope `?`. Options cite both depth constants. D15 says do not resurrect a bin that is already gone.

### Strengths
- Sequence lock is the right product decision: do not create `partnernetsoftware/rh` until a `.rh` file runs without the root package. Pain point 3 (host in the wrong crate, `register_stub_host` dummies return `-4` in `load.rs`) is verified and is the actual extractability blocker, not `cargo tree`.
- Current-state table is largely accurate: library-only `publish = false` `autobins = false`; no `agenterm-platform` dep; pipeline parse → subset → transpile → cargo → `libloading`; host API 13 / codegen 107; real host in `src/script_rh_host.rs`; `agenterm rh` via `ENGINE_SUBCOMMANDS`; rustc-pack `eval` in `script_rh_cli_main.rs`; six chassis cells; APE parked in `plan-v0.1.18.md` as a v0.2.x *workbench* research gate; name collision with “ape” = Agenterm Platform Engine vs Cosmopolitan APE is real (`plan-ape-thin-shell-dynamic-packages.md`).
- Loader vs slice split, thin layout first, no Cosmo-libc, no first-run rewrite, no default FFI/`dlcall`, no `agenterm-dyn` merge, no script-common move — all match the tree and the cited research §5 / §6.2.
- Freeze *intent* is right: embedders must not depend on rustc/Cranelift/`rh_entry`/`RH_CODEGEN_REVISION`. D15 keeping AgenTerm qualify/codegen 107 / `--worker` on AOT while switching `eval`/`run` later is a realistic migration, and `RhEngineBackend::execute` still delegates to `try_execute_rh_invocation` so D1’s rollback story is real.
- Alternatives A1–A6 are actual alternatives with rejected reasons, not a fake list. A2 (don’t turn Rhai execution back on) matches PRD_02_10’s cancellation of Rhai as the forward engine while keeping the parser.
- D17’s diagnosis of `FLEET_BRIDGE` TLS (`script_rh_host.rs`) plus `RhRunContext` TLS (`script_rh_run.rs`) as why rh cannot be a library is correct; the *goal* (no thread-local host in the product crate) is the right fix even though the IR/`Send` plan is not.
- Security table is honest that StdHost is Node-like unrestricted; authorization stays the harness (PRD_02_10). Caps cited from live host code (16 MiB, 256×4 KiB, 256 env).
- crates.io naming (`rh` taken, publish `rh-lang`, lib name `rh`) is verified and is a good call. Dual license matches `crates/agenterm-rh/Cargo.toml`.
- PR-A1 → A2 → A3 → A4 as an in-tree spine (API stub, pure interp, StdHost, example gate) is the right order *if* A2 lowers to a `Send` IR and B1 waits on A5 and does not git-pin privately from public AgenTerm.

---

## Revision Summary

Rev 2 of the design answers all 15 issues (none wontfix / needs-user-input). Headline lock changes:

- **Language 1** is a closed spec: strict `validate_ast` (no `compat_validate`), value-model mapping table, and a named `std::`/`rh::` allowlist. Command/Child are in; Fleet/GUI/TCP/HTTP/task are out.
- **Send IR:** check lowers `rhai::AST` to crate-private `IrModule` and drops it. `Backend` is crate-private. Non-goal: do not expose or `Send` rhai types.
- **Pin:** public AgenTerm does **not** git-pin a private rh repo. Path member until E1 (public → SHA pin → crates.io). wbox cite is `mgttt/agenterm` rev `c8ace42`.
- **Host:** defaulted trait methods; Language-1 names via `Host::call` with script spelling; fleet dots; `ProcessRequest` + live caps on `StdHost`/`Options`.
- **Slice ABI:** named section then sign (not EOF−64); 32 bytes; no semver/hash.
- **Windows loader:** wait-parent, quoting, Ctrl-C, exit-code forward.
- **PR sequence:** B1 waits on A4+A5; pack tests `cfg(feature = "compile")`; D does not wait on C; polyglot C3 is Later.
- **CLI:** fuel/timeout off; `entry()` rules; `exit_from_int`; no eval-envelope `?`; fleet gap **33**.

---

## Independent Gate Verification (rev 2 → rev 3)

**Verifier:** opus0, 2026-08-21. Independent of the author. `Status: addressed` was **not** trusted; each of the 15 issues was re-checked against the live tree.

### Verdict on the 15 original issues

| # | Claim | Verified against live tree | Verdict |
|---|-------|----------------------------|---------|
| 1 | `Engine: Send` via crate-private `Send` IR; no `rhai::AST` on `Engine` | `CheckedAst` survives **only** in the rejected alternative A8. `Backend` is `pub(crate) trait Backend: Send` taking `&IrModule`. Root and crate both build `rhai` **without** the `sync` feature (`Cargo.toml:48,73`; `crates/agenterm-rh/Cargo.toml` `features = ["std","internals"]`), so `rhai::AST` is genuinely `!Send` and the lowering is load-bearing, not cosmetic. | **closed** |
| 2 | Public AgenTerm does not git-pin a private rh | Every `partnernetsoftware/rh.git` occurrence is post-public (E1) or an explicit negative (PR-B2 "does not add"). wbox pin re-verified verbatim: `agenterm-platform = { git = "https://github.com/mgttt/agenterm.git", rev = "c8ace42", default-features = false }`. | **closed** (citation nit fixed, below) |
| 3 | Language 1 is one closed definition | Syntax / value model / allowlist are single-sourced. **But the frozen allowlist omitted the entire method surface** — see Issue 16. | **reopened → fixed in rev 3** |
| 4 | `Host` defaults to unsupported | All four methods (`print`, `args_len`, `arg`, `call`) carry `Err(Error::unsupported(…))` bodies; omit is real. Builtin list matches `api_validate.rs:65-66` minus the deliberate `is_def_var`/`is_shared`/`eval`/`require` trim. | **closed** (caps citation fixed, below) |
| 5 | Fleet does not follow `subset.rs` into `rh-lang` | Live coupling confirmed real: `subset.rs:7` `use crate::fleet::{…}`, `subset.rs:325` `RH_SUBSET_FLEET_SHAPE`, `expr_print.rs:348` `"std"\|"rh"\|"rhai"\|"fleet"`, `api_validate.rs:3` → `SHIPPED_SURFACE_PATHS`. Design splits at PR-A1, keeps `fleet.rs` in AgenTerm, and product `Engine::check` does not run fleet-shape. | **closed** |
| 6 | Slice ABI is not EOF−64 | EOF−64 appears only as rejected (A9) and in the "trailer is rejected" rationale. Frozen placement is `.rhslice` / `__DATA,__rhslice` / `.rhslice`, 32 bytes, no semver, no hash. | **closed** |
| 7 | B1 waits on A4 **and** A5 | PR-B1 `Depends on: **PR-A4 and PR-A5**`; A5 `cfg`-gates pack tests; A4 is a runnable example with `cargo tree` demoted to a regression check. | **closed** |
| 8 | Windows loader waits and forwards the exit code | D23 + §7 step 4: libstd quoting, shared console, `SetConsoleCtrlHandler`, `WaitForSingleObject`, `GetExitCodeProcess`, `ExitProcess`; `STILL_ACTIVE` treated as 1. PR-C1 carries the exit-3 test. | **closed** |
| 9 | `Backend` is crate-private | `pub(crate)`; listed under **Not public**; no `RustcPackBackend` in `rh-lang`; no `compile` feature on the product crate. | **closed** |
| 10 | CLI fuel/wall-time off | D22. `ScriptBudgets::default()` re-verified: `operations: 1_000_000`, `wall_time_ms: 2_000`, `expression_depth: 64` (`src/script_protocol.rs:83-94`). `RH_MAX_EXPR_DEPTH = 512` (`check.rs:11`). Both constants cited. | **closed** |
| 11 | D does not wait on C | Rollout mermaid is `A→B`, `A→D`, `B→C`, `B→E`, `D→E`. PR-D1 `Depends on: PR-B3, PR-A3 — not on C`. Polyglot is under Later. | **closed** |
| 12 | One `entry()` rule | D21 + CLI + `exit_from_int`. Live `script_exit_code` re-read (`script_rh_cli_main.rs:131-135`): `u8::try_from(i32)` → `ExitCode::FAILURE`. The design's extra i64→i32 step is correct for `rh_entry: i64`. Open Question 4 is gone. | **closed** |
| 13 | B1 does not copy script-common | Non-goals and the PR-B1 file list both name `check_many.rs` / `corpus.rs` / `agenterm-script-common` explicitly. In-tree dep re-verified (`crates/agenterm-rh/Cargo.toml`). | **closed** |
| 14 | No metrics crate in v1 | Observability is stderr + exit only; Prometheus names reserved for the AgenTerm wrapper. | **closed** |
| 15 | Factual nits | Re-counted independently: `shipped_surfaces.rs` has **76** `fleet.*`; `src/operations.rs` has **43** `script_surface: "fleet…"`; `tests/script_fleet_facade_parity.rs` allowlist array is **33** entries while its doc comment still says "32 entries" (line 441). 76−43=33 ✓. `RH_HOST_API_VERSION=13` / `RH_CODEGEN_REVISION=107` ✓ (`host_api.rs:3-4`, pinned in root `tests/rh_regression.rs:7` and `tests/rh_codegen_fixtures.rs:73`). Root `[[bin]]`s are `agenterm`, `agenterm-com`, `agenterm-cc` only ✓. Chassis `CELLS` six rows + `"unknown"` `_` arm ✓. `LuaHostFunctions` `Option<Arc<dyn Fn …>>` ✓. TLS in `script_rh_host.rs:904` and `script_rh_run.rs:65` ✓. | **closed**, with three new citation errors found — Issue 17 |

### Issue 16: the frozen name allowlist omitted every method surface (reopens Issue 3)

- **Severity**: major
- **Section**: Language 1 §3; PR-A2; PR-A3
- **Description**: Rev 2 froze the allowlist with "Unknown name → `Error::unsupported(name)` at check time", listed constructors, and listed methods for **only** `PathBuf`, `Command`, `Child`, `SystemTime`, and `Bytes`. Everything else that Language 1 can *produce* had no callable surface:
  - `std::fs::metadata` yields `HostObject "std.fs.Metadata"`, but `Metadata.is_file` / `is_dir` / `is_symlink` / `is_reparse_point` / `len` / `modified` were not frozen — so `std::fs::metadata(p).is_file()` was `unsupported`.
  - Same for `std::fs::read_dir` → `DirEntry.*` (six live methods, three live fixtures) and `Command.output` → `Output.*` (eleven live members).
  - `String` is a core `Value` with **no** methods frozen, while the live language has ten (`transpile.rs` `is_stringish_method_name`) and `String.split` is a declared shipped surface (`shipped_surfaces.rs:135`).
  - `Array` / `Map` are core `Value`s with no `push` / `insert` / `get` / `keys` / `len`, all live (`transpile.rs` `is_json_method_name`, the `keys` method call, the `len` property).
  - `FileLock` and `Duration` were mapped to `HostObject` type_ids with no statement that they have no methods.

  A frozen surface that cannot express `md.is_file()` or `s.trim()` is not implementable as frozen, and PR-A3 built to it would ship an engine that fails on its own fixtures.
- **Fix applied in rev 3**: Language 1 §3 gains two tables. Core-type methods (`String` / `Array`+`Map` / `Bytes`) are **interpreter builtins, not `Host::call`** — a `Host` implementing nothing still gets them, which also keeps the sandbox story honest. HostObject methods (`Metadata`, `DirEntry`, `Output`, plus explicit "none" for `FileLock` and `Duration`) are frozen alongside their constructors. PR-A2 and PR-A3 descriptions updated to carry the two surfaces, and the three `direntry-*-probe.rh` fixtures were added to PR-A3.
- **Status**: fixed in rev 3

### Issue 17: three citation errors that survived rev 2

- **Severity**: minor
- **Description / fix applied in rev 3**:
  1. **`RH_HOST_FS_READ_CAP` is misattributed.** Rev 2 said the caps were "copied from `src/script_rh_host.rs` `host_process_request` and `RH_HOST_FS_READ_CAP`". The constant is not in that file at all — it is `crates/agenterm-rh/src/host_api.rs:35`. Only the process/env caps come from `host_process_request` (lines 300-386). Both now cited separately with the correct paths. (This error originated in review Issue 4 and was copied into the design.)
  2. **`wbox/` is not in this tree.** It is a **sibling repo** at `/Users/cbzw032/repos/wbox`; the background table and References read as a workspace-relative path. Now marked as a sibling repo. The pin content itself is correct.
  3. **`Duration` is not a `ValueKind` variant.** Rev 2's `ValueKind` → Language 1 mapping table had a `Duration` row; `transpile.rs`'s enum has no such variant (it has `FileLock` as its last member). Row relabelled as a non-`ValueKind` value produced by `std::time::Duration::*`.
- **Status**: fixed in rev 3

### Issue 18: the private repo now exists — D1's gate wording no longer matches reality

- **Severity**: minor
- **Description**: `https://github.com/partnernetsoftware/rh` was created **private with README/LICENSE only** on 2026-08-21, after rev 2 was written. Rev 2's background table asserted "GitHub repo does not exist" and D1 said "**Do not create it until** …". Both are now false as written, and a reader could mistake the placeholder for the D1 gate having been passed.
- **Fix applied in rev 3**: the background row records the private placeholder, states it holds no language code, and states that **creating it does not advance the D1 gate**. D1 is restated as "do not **populate** it with language code until A4+A5". PR-B1 is retitled from "Create" to "Populate the private placeholder". The gate itself is unchanged: the extractability bar still lives at A4+A5, and B2 still adds no git pin.
- **Status**: fixed in rev 3

### Not re-litigated

`Backend` privacy, the named-section ABI, Windows wait-parent, `entry()`, C3-not-v1, and the Host-defaults shape were checked and stand. Open Questions 1-3 (crates.io `0.1.0` vs `1.0.0`; squash vs `filter-repo`; private-repo collaborators) are **owner calls** and were deliberately left open.

### One observation, not an issue

`agenterm-chassis-loader`'s `check_native_cell` has a `native_cell: null` escape hatch (a portable image is accepted). The rh loader in D4/§7 has **no** such hatch — unknown or mismatched cell always exits 2. That is a deliberate tightening and the right call for a runtime slice, but the design cites chassis as "same idea", so the difference is worth one line if anyone later reconciles the two loaders.
