# Lightweight specialized intelligence (`agenterm-ai.exe`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

This module records admission gates for an unassigned research direction. It
does not commit a model family, executable, or release version.

- Product boundary
  - [ ] run inference in an optional CPU-first sidecar; `agenterm.exe`
    links no model runtime and performs no inference during startup or on
    the paint thread
  - [ ] consume versioned Observable Fleet events and typed feature windows
    by default; raw PTY content requires an explicit scoped capability and
    is never persisted, uploaded, or used for training by default
  - [ ] return confidence, abstention, model ID/version/hash, feature-schema
    version, explanation, source epoch/sequence, TTL, and fallback state
  - [ ] learned output is advisory: it may observe, rank, warn, or escalate
    risk, but cannot authorize or execute a high-risk fleet action
  - [ ] deterministic command-safety rules remain authoritative and a model
    can raise risk but never override a rule denial
- Runtime and model lifecycle
  - [ ] isolate model execution in a worker with no network or terminal
    control capability and explicit CPU, memory, deadline, and concurrency
    budgets; failure immediately degrades to rules without affecting GUI
  - [ ] keep training, labeling, and evaluation outside the installed
    inference path; begin with shadow mode and human-confirmed labels
  - [ ] signed model packs declare feature schema, preprocessing,
    calibration and rejection thresholds, budgets, provenance/licenses,
    compatibility range, and fixed golden input/output vectors
  - [ ] `agenterm-softmgr.exe` installs, atomically activates, audits, and
    rolls back model and runtime components independently of the GUI
  - [ ] admit a model only after fixed Windows x64 CPU benchmarks cover
    artifact size, RSS, cold start, p95 latency, CPU load, accuracy,
    calibration, false alarms, failure isolation, and simpler baselines
- Research route, not a release commitment
  - deterministic rules and expert systems establish the first labeled
    baseline for no-progress, known errors, command risk, event priority, and
    resource thresholds
  - classic ML or small neural candidates are evaluated only when they beat
    that baseline on user-visible accuracy, latency, memory, package size, and
    false-positive cost
  - sequence-model families, including recurrent, RWKV, or state-space
    approaches, remain research candidates until reproducible portable
    Windows CPU evidence beats simpler methods
  - [ ] GPU/NPU-required models, large Transformers, installed endpoint
    training, unsigned model hot-load, default raw-PTY collection, and
    automatic high-risk actions are out of scope
