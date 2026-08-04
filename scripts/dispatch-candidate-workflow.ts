#!/usr/bin/env bun
// Dispatches the GitHub Actions workflow named "Release Candidate"
// (.github/workflows/candidate.yml) for an exact, frozen commit SHA.
//
// This only invokes that one CI workflow (the qualification/freeze build
// matrix) - it does not tag, publish, or otherwise "release" anything by
// itself, and it makes no claim about any version being an "RC". The target
// release branch is derived from --version so this stays a reusable tool
// across releases rather than being tied to one version by a hardcoded default.
//
// Usage:
//   bun scripts/dispatch-candidate-workflow.ts --version 0.1.12 --sha <40-char-sha> [--ref release/v0.1.12] [--repo mgttt/agenterm]
//
// The token is read from `gh auth token` (or the GH_TOKEN / GITHUB_TOKEN env
// vars as a fallback) at run time and is never printed, logged, or written
// to disk by this script.

interface Args {
  sha: string;
  ref: string;
  repo: string;
}

function usageAndExit(message?: string): never {
  if (message) {
    console.error(`error: ${message}`);
  }
  console.error(
    "usage: bun scripts/dispatch-candidate-workflow.ts --version <X.Y.Z> --sha <40-char-sha> [--ref release/vX.Y.Z] [--repo mgttt/agenterm]",
  );
  process.exit(1);
}

function parseArgs(argv: string[]): Args {
  let version: string | undefined;
  let sha: string | undefined;
  let refOverride: string | undefined;
  let repo = "mgttt/agenterm";
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    const value = argv[index + 1];
    switch (flag) {
      case "--version":
        version = value;
        index += 1;
        break;
      case "--sha":
        sha = value;
        index += 1;
        break;
      case "--ref":
        refOverride = value;
        index += 1;
        break;
      case "--repo":
        repo = value ?? repo;
        index += 1;
        break;
      default:
        usageAndExit(`unknown argument: ${flag}`);
    }
  }
  if (!sha) {
    usageAndExit("--sha is required (the exact 40-character commit SHA to qualify)");
  }
  if (!/^[0-9a-f]{40}$/.test(sha)) {
    usageAndExit(`--sha must be a 40-character lowercase hex commit SHA, got: ${sha}`);
  }
  if (!refOverride && !version) {
    usageAndExit("either --version <X.Y.Z> or --ref <branch> is required");
  }
  if (version && !/^\d+\.\d+\.\d+$/.test(version)) {
    usageAndExit(`--version must look like X.Y.Z, got: ${version}`);
  }
  const ref = refOverride ?? `release/v${version}`;
  return { sha, ref, repo };
}

async function resolveToken(): Promise<string> {
  const fromEnv = process.env.GH_TOKEN ?? process.env.GITHUB_TOKEN;
  if (fromEnv && fromEnv.trim().length > 0) {
    return fromEnv.trim();
  }
  const proc = Bun.spawn(["gh", "auth", "token"], {
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  if (exitCode !== 0) {
    throw new Error(
      `could not resolve a GitHub token: 'gh auth token' exited ${exitCode}: ${stderr.trim()}`,
    );
  }
  const token = stdout.trim();
  if (token.length === 0) {
    throw new Error("'gh auth token' returned an empty token");
  }
  return token;
}

async function dispatchWorkflow(args: Args, token: string): Promise<void> {
  const url = `https://api.github.com/repos/${args.repo}/actions/workflows/candidate.yml/dispatches`;
  const response = await fetch(url, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      Accept: "application/vnd.github+json",
      "X-GitHub-Api-Version": "2022-11-28",
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      ref: args.ref,
      inputs: { source_sha: args.sha },
    }),
  });

  if (response.status === 204) {
    console.log(
      `dispatched: "Release Candidate" workflow queued for ${args.sha} on ref '${args.ref}'.`,
    );
    console.log(
      `check status: gh run list --workflow candidate.yml --branch ${args.ref} --limit 3`,
    );
    return;
  }

  const body = await response.text();
  // The token never appears in a GitHub API error body, but strip
  // Authorization-shaped substrings defensively before printing regardless.
  const redacted = body.replace(/Bearer\s+[A-Za-z0-9._-]+/g, "Bearer [redacted]");
  throw new Error(
    `dispatch failed: HTTP ${response.status} ${response.statusText}\n${redacted}`,
  );
}

async function main(): Promise<void> {
  const args = parseArgs(process.argv.slice(2));
  const token = await resolveToken();
  await dispatchWorkflow(args, token);
}

main().catch((error: unknown) => {
  console.error(`error: ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
});
