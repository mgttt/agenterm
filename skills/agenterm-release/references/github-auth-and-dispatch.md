# GitHub authentication, dispatch, and observation

## Why the extra human action appeared

Before commit `b390787` on 2026-07-31, the local coordinator qualified and
packaged, created `vX.Y.Z`, and atomically pushed `main` plus the tag through
Git/GCM. The tag then triggered a runner whose ephemeral `GITHUB_TOKEN`
published the Release.

The exact-SHA architecture deliberately replaced that path:

```text
candidate.yml  workflow_dispatch(source_sha)
    -> qualify once + build six platforms + seal Candidate

release.yml  workflow_dispatch(candidate_run_id, confirmation)
    -> verify exact Candidate + create tag + promote existing bytes
```

`scripts/rhai/release.rhai publish` now rejects local publication with
`release_promotion_requires_github_workflow`.

The first post-migration Candidate required a browser click because:

- Git push succeeded through GCM, but Git transport credentials are not
  Actions REST credentials.
- `gh` was not installed/authenticated.
- the connected GitHub application exposed run/job/log/artifact reads and
  reruns, but no workflow-dispatch operation;
- `GITHUB_TOKEN` exists only inside an already started workflow.

This was a tooling gap, not proof that Candidate dispatch requires human
approval. Public Promotion still requires explicit human approval.

## Authentication domains

| Domain | Purpose | Do not assume |
|---|---|---|
| Git/GCM | fetch and push Git refs | Actions REST or `gh` is authenticated |
| connected GitHub App | only its exposed repository/Actions operations | unexposed dispatch exists |
| authenticated `gh` | GitHub API operations granted to that session | Git remote credentials are reusable |
| runner `GITHUB_TOKEN` | bounded operations inside one workflow run | it exists before dispatch |

Never query, decode, print, copy, or repurpose GCM secrets as `GH_TOKEN`.
Never embed PATs in remotes. If the selected mutation channel is unavailable,
request a precise human action.

## Dispatch fallback order

1. Connected GitHub dispatch operation, if actually exposed.
2. Already installed and authenticated `gh`:

   ```text
   gh workflow run candidate.yml --ref main -f source_sha=<exact-sha>
   ```

3. Browser human-in-loop with the exact workflow, immutable SHA, required
   fields, and expected non-publishing effect.

Do not make every `main` push run a six-platform Candidate. Candidate creation
requires explicit intent even when its mechanical dispatch is automated.

## Bounded observation

Prefer channels in this order:

1. connected authenticated GitHub application;
2. already authenticated `gh`;
3. bounded public REST fallback;
4. browser/manual inspection.

Resolve a run once and retain `run_id` plus `run_attempt`. One agent owns
observation and shares structured state. Do not repeatedly search by SHA,
branch, or workflow name.

Start polling no faster than 30 seconds. Exponentially back off with jitter to
at least two minutes while state is unchanged, reset only on meaningful
transitions, and stop at a deadline or terminal result. Honor `Retry-After`,
`X-RateLimit-Remaining`, and `X-RateLimit-Reset`. Cache unchanged results and
use ETag requests where supported. Fetch logs/artifacts only for a relevant
terminal job.

For AgenTerm Candidate runs, stop automated monitoring 75 minutes after jobs
begin unless a meaningful state transition justifies one final bounded probe.
After a browser dispatch, the human only needs to confirm that the run was
started; resolve the newest `Release Candidate` run whose event is
`workflow_dispatch` and whose head/input SHA matches the requested immutable
SHA, then retain that identity. Do not ask the human to copy a run ID unless
multiple matching runs make automatic resolution ambiguous.

Candidate validity is established by its own successful preflight, six build
jobs, aggregate job, and sealed artifact. `Workflow Observer` is required
telemetry for delivery-quality qualification, but its temporary failure is an
observation defect to repair separately, not evidence that Candidate bytes are
invalid.

The first Candidate watch used repeated 15-second anonymous REST calls. Agents
behind a shared NAT exhausted the same low IP-based allowance while the
workflow remained healthy. Rate-limit exhaustion means observation is
temporarily unavailable; it does not mean the workflow failed.

If a credential helper or diagnostic command unexpectedly hangs, terminate
only the exact processes created by that attempt and remove any generated
sensitive diagnostic log. Do not broaden process cleanup.
