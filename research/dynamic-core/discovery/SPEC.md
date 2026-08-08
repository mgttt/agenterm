# Q18 — Discovery without anointing (name → hash) — SPEC

**Criteria fixed BEFORE any code is written (decisive-experiment §3 discipline).**
Not AgenTerm product scope. Clean-room; builds on `research/dynamic-core/reuse/` (Q3),
whose content-addressed store, adapter/payload blobs, and loader are the base.

## §0 background / settled, not re-opened here

- Q3 proved content addressing gives **dedup + version coexistence + no central registry**
  for the `hash → content` direction. Its **one residue**: content addressing gives no
  `name → hash` *discovery* — "the one place anointing could re-enter."
- This experiment attacks exactly that residue. It does **not** re-measure Q3's ①②③④.

## §1 hard constraints (violation ⇒ experiment invalid)

1. **Anointing detector (病灶探测器).** Any urge to make *one* directory / registry /
   name-service that everyone must consult is the **disease this experiment detects**, not a
   feature to add. If discovery cannot avoid a single mandatory resolver, that is a
   **finding**, reported — not worked around by building the resolver.
2. No DHT, no gossip, no PKI, no package manager, no version solver. A "directory" is the
   dumbest possible thing: a text file of `name hash` lines that **anyone** can author.
3. Reuse Q3's store & blobs unchanged (same hashes). The only new thing under test is the
   `name → hash` step.

## §2 minimal experiment content

- A **directory** = a text file, lines `name <space> 16-hex-hash`. Anyone can write one.
- A **consumer** = a `prog.txt` naming a payload (line 1) + adapters (rest) by **name**,
  plus a `trust.txt` naming *which directory this consumer trusts*.
- `loader.rs`, one source, **packed twice** (isolate the discovery cost as a clean Δ):
  - default (`loader_hash`): reads `manifest.txt` of **hashes** — identical to Q3's loader
    = the **build-time-pinned** baseline (candidate 3: no runtime discovery at all).
  - `--cfg dc_discover` (`loader_disc`): reads `trust.txt` → directory → resolves the
    names in `prog.txt` to hashes at **runtime** (candidate 1: multiple directories).
- Two directories `dir_a.txt`, `dir_b.txt` map the **same name** `fileio` to **different
  hashes** (v1 full-read vs v2 truncated-read). Same `prog.txt`, swap only `trust.txt`.

## §3 criteria (pinned; no post-hoc edits)

| # | criterion | type | what decides it |
|---|---|---|---|
| ① | **Boolean gate.** Two consumers, two non-conflicting `name→hash` maps, resolve the **same name** `fileio` to **different hashes**, both load & **run correctly** | boolean | consumer A prints `len=0023` (v1), consumer B prints `len=0008` (v2), same `prog.txt`, only `trust.txt` differs. Fail ⇒ multi-directory discovery does not even function |
| ② | **Anointing check (MAIN).** Does the chosen mechanism have anything that *must exist, everyone must use, therefore must cover everyone*? Version-coexistence discriminator (Q3's): can the **same name**'s two incompatible hashes be used by two consumers **simultaneously**? | boolean/list | if two incompatible `fileio` hashes coexist with no mandatory shared directory ⇒ no anointing. If "choosing whom to trust" turns out to require one blessed directory ⇒ soft anointing, reported |
| ③ | **Cost.** Mechanism size (`loader_disc` − `loader_hash`, same 口径), and **is discovery runtime or build-time?** | slope/intercept | byte Δ of the name-resolution layer; whether that Δ is *removable* by resolving at build time |
| ④ | **Residue.** To what level does the chosen scheme drive anointing risk, and what remains? | list | "must trust *some* publisher / *some* build choice" = **local** choice, not **global** mandate — state the distinction precisely |
| ⑤ | **vs build-time pinning (MAIN, may flip the whole experiment).** Is discovery a **runtime** problem at all? Q3's `manifest.txt` already names hashes = build-time-pinned, **zero runtime discovery**. If candidate 3 holds, ①②③④ about a runtime mechanism become **mostly irrelevant** and Q3's residue is a **build-time** concern, not a runtime one | judgement | does `loader_hash` (no discovery code) achieve the same two outcomes as `loader_disc`? If yes, discovery is movable out of runtime entirely |

## §4 decision tree

```
1. ⑤ first: does loader_hash (build-time-pinned hashes, ZERO discovery code) achieve
   the same two divergent outcomes as loader_disc?
   - YES → discovery is a BUILD-TIME concern; runtime has no discovery residue.
     ①②③ still reported (they show the runtime variant also works & its price),
     but the VERDICT is driven by ⑤ + ②.
2. ② main: is there a single mandatory resolver anywhere on the used path?
   - NO single mandatory directory + incompatible hashes coexist → 无钦定 (no anointing).
   - Reducible only to "trust SOME publisher/build" → LOCAL choice, not global mandate.
3. ① boolean gate: if the two consumers cannot both run correctly → discovery broken, stop.
kill criterion: if ANY working discovery path requires one directory everyone must use
   → report "discovery cannot avoid anointing", do NOT build the mandatory resolver.
time box: stop when ①②⑤ have numbers/verdicts. No DHT/gossip/PKI/package-manager.
```

## §6 excluded options

| option | why excluded |
|---|---|
| DHT / gossip / content routing | that is a distributed-systems research program, not the anointing question |
| PKI / signature chains | candidate 2 ("name carries trust anchor") is *analyzed*, not built — building it slides into a name service |
| version solver / SAT | Q3 §1.1 excluded it; a solver is the package-manager disease |

## §7 not answered here

- Network fetch of blobs/directories (the store is local, as in Q3).
- Revocation, staleness, directory freshness — those are name-service concerns (the slide).
- Cryptographic publisher identity (candidate 2) — analyzed only.
