# Lightweight specialized intelligence (`agenterm-ai.exe`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

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
- Capability route
  - [ ] A0 rules and expert systems cover no-progress/stall detection,
    known error and exit patterns, command risk, event priority, resource
    thresholds, and deterministic degradation
  - [ ] A1 benchmark XGBoost, random forest, constrained SVM, and small MLP
    candidates in shadow mode for anomaly/error classification, resource
    warning, and event prioritization; model and runtime size are measured,
    not assumed from the algorithm name
  - [ ] A2 benchmark small GRU first and LSTM second for typed event rhythm,
    prolonged no-progress, resource trend, and context-exhaustion
    prediction; recurrent state is epoch-bound and resets on restart or gap
  - [ ] A3 keeps sub-million-parameter RWKV-small as research only:
    constant context-state memory does not include weights, vocabulary, or
    runtime, and it must beat simpler models on the Windows CPU baseline
  - [ ] A4 keeps Mamba-small as research only until a reproducible portable
    Windows CPU kernel and export path beat GRU and classic-ML baselines
  - [ ] GPU/NPU-required models, large Transformers, installed endpoint
    training, unsigned model hot-load, default raw-PTY collection, and
    automatic high-risk actions are out of scope
