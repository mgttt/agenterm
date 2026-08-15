# Goal: freeze a thin Shell-L1 so development stops waiting on six-cell CI

Status: **active**  
Execution: [`refactor-shell-l1-l2-l3.md`](refactor-shell-l1-l2-l3.md)  
Surface: [`shell-l1-surface.json`](shell-l1-surface.json)  
Paths: repository-relative or `~/...` only.

## Outcome

Kernel shell (Shell-L1) is thin and named. Six-cell Candidate runs because L1
changed, not because an app, a catalog row, or a cu hand changed.

Apps (Shell-L3) call a versioned Host ABI (Shell-L2). L2 updates do not rebuild
the Base PE.

## This increment (W0)

- Plan tree exists.
- L1 path surface is machine-readable.
- ARCHITECTURE and PRD point at the tree without claiming the PE is split.

## Next (W1+)

Wire the surface into CI intent, then freeze Host ABI names, then ship one
L2 artifact that does not invoke Base Candidate.

## Non-goals

Electron in the official PE. App-level `dlcall`. A second platform inside dyn.
Renaming the script L1/L2/L3 documents away.
