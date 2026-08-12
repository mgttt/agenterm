# Pre-product-split CI archive

This directory preserves the last active monolithic feedback workflows before
CI ownership split between the `agenterm` workbench and `agenterm-con`.

- `ci.yml` mixed shared platform checks, workbench behavior, Script Runtime,
  Control Center, packaging probes, and con coverage in one status.
- `win-full-gate.yml` was the manually dispatched integrated Windows gate.

These files are historical evidence only. They live outside
`.github/workflows/` so GitHub Actions cannot execute them. Candidate and
Release authority remains in the active workflows; this archive must never be
used as a release prerequisite.
