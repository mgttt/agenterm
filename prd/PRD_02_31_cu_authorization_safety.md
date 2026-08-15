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

- [~] Legacy `--grant` / `AGENTERM_CU_GRANT` selection is now strict and
  single-source: a present CLI value wins without union or fallback, and an
  empty or unknown scope returns typed `invalid_authorization` without echoing
  the supplied token. The Windows smoke supplies an environment `actuate`
  scope behind an explicit CLI `observe` placement attempt and proves the
  action remains refused with unchanged bounds. These remain ephemeral
  process inputs, not persisted or target-bound grants.
- [~] SSH and VNC worker environment forwarding now reserves the complete
  case-insensitive `AGENTERM_CU_GRANT*` and `AGENTERM_CU_AUTH*` namespaces.
  Caller-supplied matches fail typed before SSH spawn or VNC handshake, the
  value is not echoed, and inherited selectors are removed from child process
  environments. Existing raw scope forwarding is still legacy ephemeral
  behavior, not a target-bound delegation protocol.
- [~] A persisted bounded grant-store core now owns validated create/list,
  pre-effect attempt reservation, revocation, expiry/not-before, exact
  target+tier+session matching, generation conflict detection, and corrupt
  store refusal. Observe grants are capped at 24 hours / 10,000 uses; any
  grant containing Actuate is capped at 1 hour / 100 uses, and one-shot means
  exactly one attempt even when the downstream mechanism fails. RDP cannot
  receive a grant while its transport is unavailable. Publication uses a
  flushed same-directory replacement snapshot. Cooperating writers now take a
  zero-wait platform lock on a stable sibling sidecar, re-read generation while
  holding it, and retain the guard through replacement and durability handling;
  contention is typed and publishes nothing. This closes the local
  compare-to-rename race without claiming protection from non-cooperating
  writers, hostile sidecar replacement, cross-session Windows callers or
  filesystems without coherent locking. This is not yet the production
  authorization path: CLI/provider integration, verified target identity and
  session-nonce invalidation remain open.
- [~] A sealed `TargetBinding` contract now separates opaque provider identity
  and exact desktop-session identity from routing material. Current, SSH and
  VNC fail typed when no crate-owned verified provider is available; RDP stays
  `unsupported` even if a provider is offered. The public resolver accepts no
  hostname, IP, user, port, `DISPLAY` or arbitrary binding-string constructor.
  `agenterm-platform` now provides the first Windows `current` identity
  mechanism: enrollment creates one locked, current-user-only 32-byte install
  key, while load/query never rotate missing or corrupt state. The session
  digest binds the opaque provider to token SID, authentication and terminal
  session identifiers, positive WTS active/logon evidence, and the current
  input desktop. Session zero, a disconnected or changed desktop, unsafe key
  state, and unavailable proof fail typed. The digest is only an opaque
  equality identifier, not an authenticator. CU now adapts those two opaque
  32-byte identities into its sealed, fixed-prefix target/session binding;
  resolution never creates state, while a separate explicit enrollment call
  owns first installation. Linux and macOS providers remain explicitly
  unsupported. The verified binding is not yet accepted by the grant-store
  schema, CLI or per-command executor decision, so no persisted grant is
  executable through this mechanism yet.
- [x] The staged Windows x86_64 `cu-windows-smoke` proves an observe-only
  `window-place` is typed `refused` and leaves the owned fixture bounds
  unchanged. The authorized call writes exactly one `attempt` and one `ok`
  JSONL record through one retained audit handle; each record names the
  `authorized` decision and process-bounded authority scope. Independent
  window enumeration confirms the reported placement, and the smoke rejects
  credential-like fields from the published audit records.
- [ ] This checkpoint does not prove per-target expiry/revocation, every
  actuation verb, credential absence from every published artifact, Windows
  ARM64, another OS, or the required remote-transport security review. The
  module therefore remains planned/partial rather than shipped.
