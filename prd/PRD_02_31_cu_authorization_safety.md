# `agenterm-cu` authorization, safety and audit

Parent: [Computer-use foundation (`agenterm-cu`)](PRD_02_28_agenterm_cu.md)

Computer-use is a high-risk capability face: full desktop actuation plus remote
transports is exactly the shape used for lateral movement. This module owns the
authorization model, the audit record, and the refusal semantics. It exists so
that the capability face cannot ship before its control face.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Why this is not the script-engine posture

- [ ] `agenterm-cu` does **not** inherit the unrestricted local runtime posture
  of [10 script engines](PRD_02_10_rhai_scripting.md). That posture is
  deliberate for a local automation runtime the invoking user already fully
  controls. It is not appropriate for a surface whose defining feature is
  actuating other machines.
- [ ] the difference is the target, not the trust level of the user: a command
  set that reaches beyond the invoking machine needs an explicit grant per
  target, not ambient authority inherited from the process that started it.

## Authorization model

- [ ] every action is authorized before execution. There is no code path that
  actuates a target without passing the authorization decision.
- [ ] authority is granted per target and is explicit, bounded and revocable.
  Possession of a target reference is not by itself authority to act on it.
- [ ] remote credentials, secrets and session material are isolated from
  command payloads, logs, snapshots, screenshots and error text. Redaction is a
  property of the evidence path, not something each call site remembers.
- [ ] the default posture is least capability. A newly reachable target grants
  observation before actuation, and actuation requires a distinct explicit
  grant.
- [ ] a denied action fails typed and locally. It never partially executes, and
  it never falls back to a lower-fidelity path that achieves the same effect.

## Audit

- [ ] every authorized action produces an observable record identifying target,
  command, decision, outcome and time, sufficient to reconstruct what was done
  to which machine.
- [ ] the audit record is machine-readable and survives the session that
  produced it.
- [ ] failure to record is failure to act: if the audit path is unavailable, the
  action does not execute.

## Refusal semantics

- [ ] refusal is typed and distinguishable from mechanism failure. A caller can
  always tell "you are not allowed" from "this target cannot do that" from
  "this attempt failed".
- [ ] no refusal is silently retried through another tier, transport or
  coordinate fallback.

## Delivery gate

- [ ] no tier of [30](PRD_02_30_cu_targets_transports.md) may be claimed shipped
  before this module's authorization, audit and refusal requirements are proven
  for that tier. The `current` tier is included: local actuation is not exempt
  because it is local.
- [ ] the evidence is a public black-box journey proving an unauthorized action
  is refused, a revoked grant stops taking effect, an authorized action is
  recorded, and credential material is absent from every published artifact.
- [ ] a security review of this surface is required before any remote transport
  tier is claimed, and it belongs to the release gate rather than to the
  authoring agent's own judgment.

## Windows current checkpoint

- [x] The staged Windows x86_64 `cu-windows-smoke` proves an observe-only
  `window-place` is typed `refused` and leaves the owned fixture bounds
  unchanged. The authorized call writes exactly one `attempt` and one `ok`
  JSONL record through an isolated audit path, and independent window
  enumeration confirms the reported placement.
- [ ] This checkpoint does not prove per-target expiry/revocation, every
  actuation verb, credential absence from every published artifact, Windows
  ARM64, another OS, or the required remote-transport security review. The
  module therefore remains planned/partial rather than shipped.
