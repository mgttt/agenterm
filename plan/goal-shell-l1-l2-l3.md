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

## Done

- W0: L1 path surface named.
- W1: plan states the pack-not-compile loop; `scripts/shell-compose-product.py`
  copies frozen six-cell L1 loaders plus L2/L3 into a deterministic archive
  without cargo; `scripts/shell-compose-product-test.py` proves L1 SHA
  identity, determinism, and a PATH with no working cargo.

This increment proves the economic model. It does **not** split the live
`agenterm` PE.

## Next (later waves)

Host ABI name table, cu as L2, first real L2 payload through the composer,
then v0.1.18 `.agp` as L3.

## Non-goals

Electron in the official PE. App-level `dlcall`. A second platform inside dyn.
Renaming the script L1/L2/L3 documents away.
