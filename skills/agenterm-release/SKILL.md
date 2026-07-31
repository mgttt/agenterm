---
name: agenterm-release
description: Operate and diagnose AgenTerm's exact-SHA Release Candidate and approved byte-promotion workflows. Use for Candidate creation, release rehearsal, GitHub Actions monitoring, release authentication or dispatch failures, Promotion approval, tag/Release publication, or delivery-latency investigation in the AgenTerm repository.
---

# AgenTerm Release

## Establish authority

Read these repository-owned contracts before acting:

1. `AGENTS.md`, especially Development loop, formal delivery, and GitHub
   Actions observation.
2. `prd/PRD_02_17_delivery_quality.md`.
3. `.github/workflows/candidate.yml`, `.github/workflows/release.yml`, and
   `.github/workflows/release-integrity.yml`.
4. `scripts/rhai/release.rhai` and `agenterm.tasks.json`.

Read `references/github-auth-and-dispatch.md` when authentication, workflow
dispatch, monitoring, rate limits, or human approval is involved.

Treat the current files and remote run state as authoritative. Do not infer the
delivery topology from an older release or from Git push behavior.

## Candidate workflow

1. Synchronize and inspect `origin/main`; preserve other platform agents'
   commits.
2. Require the exact lowercase 40-character current `origin/main` HEAD and a
   successful ordinary CI run for that SHA. Candidate currently rejects a
   historical main ancestor because `workflow_dispatch` controller identity,
   provenance, and Promotion must remain one unambiguous commit.
3. Run local lint and only the owning policy/fixture tests before dispatch.
4. Dispatch `candidate.yml` for that exact SHA through an actually available,
   authenticated Actions capability.
5. If dispatch is unavailable, stop and give the human the exact workflow
   link, SHA, fields, and non-publishing effect. Never extract a GCM secret to
   manufacture REST authentication.
6. After dispatch, resolve the newest `Release Candidate` run matching the
   exact SHA and `workflow_dispatch` once; record its `run_id` and
   `run_attempt`. If a human dispatched it, their `已启动` confirmation is
   enough to begin that one-time resolution; they do not need to copy an ID.
7. Observe the retained run ID through one bounded observer, with a 75-minute
   deadline after jobs begin. Verify preflight, all six platform parts, the
   single Windows stress qualification, aggregate, and sealed Candidate
   artifact. The workflow's success and sealed artifact are hard Candidate
   validity requirements. The separate read-only Workflow Observer is required
   delivery-quality evidence but an observer outage does not turn a valid
   Candidate into a failed build.
8. On failure, fetch only the failed job log/artifact, fix the owning cause,
   validate locally, push a coherent increment, and create a new Candidate.
   Never rebuild silently during Promotion.

Candidate dispatch is mechanical and may be automated after an explicit
exact-SHA candidate request. It creates no tag or public Release.

## Promotion workflow

Promotion is a separate human authority boundary.

1. Do not dispatch `release.yml` until the user explicitly approves public
   publication for the exact Candidate.
2. Bind `candidate_run_id`, source SHA, version, expected tag, artifact
   identity, expiry, and confirmation `publish-vX.Y.Z`.
3. Require the configured `release` environment approval when available.
4. Verify that Promotion performs no Cargo build, test, package, signing,
   notarization, or overwrite.
5. A retry may resume only an exact-SHA tag and unpublished matching draft.
   Verify its Candidate marker, exact title/body/body hash, and every retained
   asset by allowlisted name, size, and SHA-256; upload only missing assets
   without overwrite.
6. Verify the tag points to the Candidate SHA, the published Release contains
   the exact allowlisted bytes, and `Release asset integrity` succeeds.
7. Report remaining risk and links; never claim success from a draft,
   incomplete matrix, or merely green tag-independent CI.

Without explicit public-release approval, stop after Candidate verification.

## Local coordinator

Use `release.cmd --rehearse` for read-only validation/rehearsal.
`release.cmd` intentionally refuses local publication. Do not restore the old
local tag-push path merely to avoid workflow dispatch.

Keep Candidate/Promotion policy tests line-ending independent. Pin every source
file whose bytes enter cross-platform provenance to LF in `.gitattributes`;
otherwise Windows and Unix can hash different working-tree bytes for one Git
commit.

## Delivery discipline

- Push small, coherent, reviewed progress to `main` early so Linux/macOS agents
  can rebase and test.
- Keep Candidate and Promotion permissions least-privileged and actions pinned
  to immutable commits.
- Keep observation read-only and bounded; observation loss is not workflow
  failure.
- Preserve exact-SHA receipts, hashes, SBOM, provenance, artifact allowlists,
  expiry, and no-overwrite semantics.
- Never create a tag or GitHub Release without explicit user approval.
