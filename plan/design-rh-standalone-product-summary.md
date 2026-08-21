# Gate verification: partnernetsoftware/rh design (rev 4)

**Verifier:** opus0, 2026-08-21. Independent gate-level re-verification of `design-rh-standalone-product.md` against the live tree. `Status: addressed` was not trusted; each of the 15 review issues was re-checked, and each `Closed` below has a file:line behind it in `design-rh-standalone-product-review.md` § *Independent Gate Verification*.

## 复验结论：可通过 (pass) — rev 3 carried my edits, rev 4 carries the owner lock

Rev 2 was substantively correct on all nine items the owner flagged. One major gap and three citation errors survived the author's self-review; all four were fixed directly in the design rather than reopened as blockers. No blocking issue remains. Nothing in `crates/`, `src/`, or `prd/` was touched; no repo was created; no `Cargo.toml` was changed.

## The nine flagged items

| Item | Verdict | Evidence |
|------|---------|----------|
| `Engine: Send` vs `rhai::AST` | **Closed** | `CheckedAst` survives only in rejected alt A8. `pub(crate) trait Backend: Send` takes `&IrModule`. rhai is built **without** the `sync` feature in both `Cargo.toml:48,73` and the crate, so `AST` really is `!Send` — the lowering is load-bearing. |
| Language 1 still three definitions? | **Closed as three; reopened as incomplete** → fixed (Issue 16) | Syntax/value-model/allowlist are single-sourced, but the frozen allowlist had no method surface. |
| `Host` defaults unsupported | **Closed** | All four methods carry `Err(Error::unsupported(…))`. Builtins match `api_validate.rs:65-66` minus a deliberate `eval`/`require` trim. |
| Fleet following `subset.rs` into `rh-lang` | **Closed** | Live coupling is real (`subset.rs:7`, `:325`; `expr_print.rs:348`; `api_validate.rs:3`); design splits at A1, `fleet.rs` stays, product check drops `RH_SUBSET_FLEET_SHAPE`. |
| Slice ABI still EOF−64 | **Closed** | EOF−64 only as rejected (A9). Frozen: `.rhslice` / `__DATA,__rhslice` / `.rhslice`, 32 bytes, no semver, no hash, section-then-sign. |
| Public AgenTerm git-pinning a private repo | **Closed** | Every `rh.git` mention is post-public (E1) or an explicit negative (B2). wbox pin re-verified verbatim. |
| Windows loader wait + exit-code forward | **Closed** | D23 + §7: libstd quoting, shared console, `SetConsoleCtrlHandler`, `WaitForSingleObject`, `GetExitCodeProcess`, `ExitProcess`; exit-3 test in C1. |
| `Backend` crate-private | **Closed** | `pub(crate)`, listed under **Not public**, no `RustcPackBackend`, no `compile` feature on the product crate. |
| PR-B1 depends on A4+A5 | **Closed** | Stated as `**PR-A4 and PR-A5**`; A5 `cfg`-gates pack tests; A4 is a runnable example, `cargo tree` demoted to a regression check. |

## What I changed in rev 3

1. **Issue 16 (major) — the frozen allowlist omitted every method surface.** Rev 2 froze constructors and said "unknown name → `unsupported`", but listed methods for only `PathBuf`/`Command`/`Child`/`SystemTime`/`Bytes`. So `std::fs::metadata(p).is_file()`, `s.trim()`, `arr.push(x)`, and `Command.output().success()` were all unsupported in a surface declared frozen — unimplementable as written, and PR-A3 built to it would fail its own fixtures. Added two tables: core-type methods (`String`/`Array`/`Map`/`Bytes`) as **interpreter builtins, not `Host::call`** (so a no-op `Host` still gets them), and HostObject methods (`Metadata`, `DirEntry`, `Output`, plus explicit "no methods" for `FileLock` and `Duration`). Names taken from `transpile.rs` `is_stringish_method_name` / `is_json_method_name` and `shipped_surfaces.rs`. PR-A2/A3 updated; three `direntry-*-probe.rh` fixtures added to A3.
2. **Issue 17 (minor) — three citation errors.** `RH_HOST_FS_READ_CAP` is at `crates/agenterm-rh/src/host_api.rs:35`, **not** in `src/script_rh_host.rs` (the error came from review Issue 4 and was copied into the design); `wbox/` is a **sibling repo** at `/Users/cbzw032/repos/wbox`, not a workspace path; `Duration` is **not** a `ValueKind` variant.
3. **Issue 18 (minor) — the private placeholder now exists.** `partnernetsoftware/rh` was created private, README/LICENSE only, on 2026-08-21. Rev 2's "GitHub repo does not exist" and D1's "do not **create** it until…" are both now false as written. Background row records the placeholder and states that **creating it does not advance the D1 gate**; D1 restated as "do not **populate** with language code until A4+A5"; PR-B1 retitled Create → Populate. The gate is unchanged and B2 still adds no git pin.

## Independently re-counted

`shipped_surfaces.rs` **76** `fleet.*`; `src/operations.rs` **43** `script_surface: "fleet…"`; parity allowlist array **33** entries against a doc comment still saying "32" (`tests/script_fleet_facade_parity.rs:441`). 76−43=33 ✓. `RH_HOST_API_VERSION=13` / `RH_CODEGEN_REVISION=107` ✓. Root `[[bin]]`s are `agenterm`/`agenterm-com`/`agenterm-cc` only ✓. Chassis six `CELLS` + `"unknown"` ✓. `ScriptBudgets::default()` 1e6 ops / 2000 ms / depth 64 ✓, `RH_MAX_EXPR_DEPTH = 512` ✓. TLS at `script_rh_host.rs:904` and `script_rh_run.rs:65` ✓.

## Owner lock — all three closed, nothing open

**Closed 2026-08-21 by grok-mcu (D24 / D4):**

1. crates.io first public version: **0.1.0**.
2. Populate private repo: **squash/copy + NOTICE** (no `git filter-repo` of AgenTerm history).
3. Collaborators: **org members only**.

Chassis `native_cell: null` escape is **not** copied into the rh loader (D4). Unknown or mismatched cell always exits 2.

Gate unchanged by the lock: **no language code enters the private repo until PR-A4 and PR-A5 are green**, and PR-B2 still adds no git pin. Design is closed; next move is the A chain.
