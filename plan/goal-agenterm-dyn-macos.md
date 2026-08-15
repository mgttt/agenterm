# Goal: first-class `agenterm-dyn` on macOS (wave 3)

Status: active  
Owner: this session (`agenterm-osx`) + exclusive-file subagents **in background**  
CWD: repository root. Paths: repo-relative or `~/...` only.  
Git identity: `agenterm-osx <agenterm@mgttt.com>`.

## Outcome

Waves 1–2 are on `main` (40 then 43 probes; loaded-symbol Darwin ioctl).
This Darwin host just ran `cargo test -p agenterm-dyn`: **106 passed**.
Wave 3 adds more Darwin-only integer/ptr facts, a few more header-alias
rejects, and honest CU-adjacent notes. Still not a fourth engine.

## Invariants

- No C, no libffi, no JIT, no lambda, no cu/platform import.
- Do not fake live probes on Linux/Windows.
- Subagents do **not** commit. Primary owns I/P as `agenterm-osx`.
- Commit-pull-rebase-push on each coherent increment. Exact pathspec.
- Background only. Private `CARGO_TARGET_DIR`: `target/dyn-harden3`,
  `target/dyn-probes3`, `target/dyn-cuadj`.
- Do **not** re-do ioctl, `sysctlbyname`, `mach_absolute_time`,
  `getprogname`, `issetugid`, `_NSGetExecutablePath`, `proc_pidpath`,
  `arc4random`. Those shipped.

## DAG

```text
G  this goal
├── A  more C-header aliases     files: tests/errors.rs only
├── B  three more Darwin probes  files: src/hosts.rs (system_probes only),
│                                tests/hosts.rs (system_probes asserts),
│                                tests/macos_probes.rs (append),
│                                examples/{clock-gettime-nsec-np,sysctl,
│                                mach-host-self}.md (new)
├── C  CU-adjacent macos notes   files: src/hosts.rs CU-adjacent constants
│                                + catalog tests at the bottom of hosts.rs
│                                (do not touch system_probes)
└── I/P  primary: README/PRD if needed, test, commit, rebase, push
```

A and B share no files. B and C both touch `src/hosts.rs` in **disjoint
regions**: B = `LINUX_SYSTEM_PROBES` / `MACOS_SYSTEM_PROBES` /
`PLACEHOLDER_SYSTEM_PROBES` / `HostCell.system_probes` length; C = only
`MACOS_WINDOW_LIST` / `MACOS_FOCUS` / `MACOS_GET_TEXT` and the
`#[cfg(test)]` catalog tests at the file bottom. Do not reformat the
whole file.

## Leaves

### A — harden

Add to the existing C-alias reject list (same test shape, no library load):
`ptrdiff_t`, `rlim_t`, `dev_t`, `ino_t`, `clockid_t`, `sigset_t`.
Do not accept as aliases.

### B — probes (43 → 46)

| name | Darwin | Linux / Windows |
|------|--------|-----------------|
| `clock_gettime_nsec_np` | live `clock_gettime_nsec_np` | Placeholder |
| `sysctl` | live `sysctl` | Placeholder |
| `mach_host_self` | live `mach_host_self` | Placeholder |

Live tests (`#[cfg(macos)]` append):

- `clock_gettime_nsec_np(CLOCK_UPTIME_RAW)` as `u64`; two calls monotonic.
- `sysctl` `CTL_HW`/`HW_NCPU` into caller-owned `i32` + `size_t`; rc 0;
  ncpu >= 1; agree with `libc::sysctl` or `sysctlbyname("hw.ncpu")`.
- `mach_host_self` returns a non-zero port (`u32`); second call same value.

If a symbol is missing, omit that row. One example md per new name.

### C — CU-adjacent notes

`MACOS_WINDOW_LIST` / `FOCUS` / `GET_TEXT` still say “planned hand”.
cu `window-place` + AX is live on this host. Update **notes only**
(still script data, no import of cu/platform): say cu owns the live AX
hand; dyn names ApplicationServices / AX symbols only. Add/adjust a
hosts.rs unit test that the macos notes mention `AX` and do not say
“planned”. Do not change linux/windows facts.

## Non-goals

Windows live. JIT. General variadic FFI. `getloadavg`. Re-opening ioctl.
