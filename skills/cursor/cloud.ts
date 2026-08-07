#!/usr/bin/env bun
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

/**
 * Cursor Cloud Agents CLI — adaptive auth + thin REST wrapper.
 *
 * Auth (first hit wins; never printed):
 *   1) process.env.CURSOR_API
 *   2) ~/env.jsonl  (and common host paths) — line objects:
 *        { "CURSOR_API": { "api_key": "crsr…", "type": "llm" } }
 *        { "CURSOR_API": "crsr…" }
 *        { "key": "CURSOR_API", "value": "crsr…" }
 *
 * Usage:
 *   bun skills/cursor/cloud.ts me
 *   bun skills/cursor/cloud.ts list [--active] [--env NAME] [--all] [--json]
 *   bun skills/cursor/cloud.ts get <bcId|name>
 *   bun skills/cursor/cloud.ts create --prompt TEXT [--name N] [--env NAME] [--repo URL] [--ref REF] [--model ID]
 *   bun skills/cursor/cloud.ts chat --to <bcId|name> --prompt TEXT [--from LABEL]
 *   bun skills/cursor/cloud.ts runs <bcId|name> [--limit N]
 *   bun skills/cursor/cloud.ts run get <bcId|name> <runId>
 *   bun skills/cursor/cloud.ts cancel <bcId|name> [runId]
 *   bun skills/cursor/cloud.ts archive|unarchive <bcId|name>
 *   bun skills/cursor/cloud.ts models
 *   bun skills/cursor/cloud.ts auth-check   # loads key, calls /me (no secret echo)
 *
 * Desensitize: never logs CURSOR_API, Basic headers, or full key material.
 */

const API_BASE = (process.env.CURSOR_AGENT_API_BASE ?? "https://api.cursor.com/v1").replace(
  /\/$/,
  "",
);

const EXIT = {
  ok: 0,
  busy: 1,
  usage: 2,
  auth: 3,
  network: 4,
  api: 5,
} as const;

// ─── auth ───────────────────────────────────────────────────────────────────

type AuthSource = "env:CURSOR_API" | `file:${string}`;

function candidateEnvJsonlPaths(): string[] {
  const home = homedir();
  const paths = [
    process.env.CURSOR_ENV_JSONL,
    home ? join(home, "env.jsonl") : "",
    "D:/env.jsonl",
    "D:\\env.jsonl",
  ].filter(Boolean) as string[];
  // Keep native separators; also try slash form for mixed hosts.
  const expanded = paths.flatMap((p) => [p, p.replace(/\\/g, "/")]);
  return [...new Set(expanded)];
}

function extractKeyFromJsonValue(value: unknown): string | null {
  if (typeof value === "string" && value.trim()) return value.trim();
  if (value && typeof value === "object") {
    const o = value as Record<string, unknown>;
    for (const k of ["api_key", "value", "token", "apiKey", "key", "CURSOR_API"]) {
      const v = o[k];
      if (typeof v === "string" && v.trim().length >= 20) return v.trim();
    }
  }
  return null;
}

function loadKeyFromEnvJsonl(path: string): string | null {
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch {
    return null;
  }
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || !line.includes("CURSOR_API")) continue;
    try {
      const obj = JSON.parse(line) as Record<string, unknown>;
      if ("CURSOR_API" in obj) {
        const key = extractKeyFromJsonValue(obj.CURSOR_API);
        if (key) return key;
      }
      if (obj.key === "CURSOR_API") {
        const key = extractKeyFromJsonValue(obj.value ?? obj);
        if (key) return key;
      }
    } catch {
      // skip non-JSON lines
    }
  }
  return null;
}

function resolveApiKey(): { key: string; source: AuthSource } {
  const fromEnv = process.env.CURSOR_API?.trim();
  if (fromEnv) return { key: fromEnv, source: "env:CURSOR_API" };

  for (const path of candidateEnvJsonlPaths()) {
    const key = loadKeyFromEnvJsonl(path);
    if (key) {
      // inject into process for child tools that only read env
      process.env.CURSOR_API = key;
      return { key, source: `file:${path}` };
    }
  }
  fail(
    EXIT.auth,
    "CURSOR_API not found. Set env CURSOR_API (cloud VM) or add a CURSOR_API line to ~/env.jsonl (local).",
  );
}

function redact(text: string): string {
  return text
    .replace(/Authorization:\s*Basic\s+[A-Za-z0-9+/=]+/gi, "Authorization: Basic <redacted>")
    .replace(/Bearer\s+[A-Za-z0-9._~+/=-]+/gi, "Bearer <redacted>")
    .replace(/crsr[A-Za-z0-9]{10,}/g, "crsr<redacted>")
    .replace(/"api_key"\s*:\s*"[^"]+"/gi, '"api_key":"<redacted>"')
    .replace(/"apiKey"\s*:\s*"[^"]+"/gi, '"apiKey":"<redacted>"')
    .replace(/"token"\s*:\s*"[^"]+"/gi, '"token":"<redacted>"');
}

function fail(code: number, message: string): never {
  console.error(redact(message));
  process.exit(code);
}

function usage(message?: string): never {
  if (message) console.error(`error: ${message}`);
  console.error(`usage:
  bun skills/cursor/cloud.ts me
  bun skills/cursor/cloud.ts auth-check
  bun skills/cursor/cloud.ts list [--active] [--archived] [--env NAME] [--all] [--limit N] [--json]
  bun skills/cursor/cloud.ts get <bcId|name>
  bun skills/cursor/cloud.ts create --prompt TEXT [--name N] [--env NAME] [--repo URL] [--ref REF] [--model ID] [--json]
  bun skills/cursor/cloud.ts chat --to <bcId|name> --prompt TEXT [--from LABEL] [--json]
  bun skills/cursor/cloud.ts runs <bcId|name> [--limit N] [--json]
  bun skills/cursor/cloud.ts run-get <bcId|name> <runId> [--json]
  bun skills/cursor/cloud.ts cancel <bcId|name> [runId]
  bun skills/cursor/cloud.ts archive <bcId|name>
  bun skills/cursor/cloud.ts unarchive <bcId|name>
  bun skills/cursor/cloud.ts models [--json]

Auth: CURSOR_API env, else ~/env.jsonl (see file header). Never prints the key.`);
  process.exit(EXIT.usage);
}

// ─── HTTP ───────────────────────────────────────────────────────────────────

async function api(
  method: string,
  path: string,
  body?: unknown,
): Promise<{ status: number; json: unknown; text: string }> {
  const { key } = resolveApiKey();
  const auth = Buffer.from(`${key}:`, "utf8").toString("base64");
  const url = path.startsWith("http") ? path : `${API_BASE}${path.startsWith("/") ? "" : "/"}${path}`;
  let res: Response;
  try {
    res = await fetch(url, {
      method,
      headers: {
        Authorization: `Basic ${auth}`,
        Accept: "application/json",
        ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
      },
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
  } catch (err) {
    fail(EXIT.network, `network error: ${err instanceof Error ? err.message : String(err)}`);
  }
  const text = await res.text();
  let json: unknown = null;
  try {
    json = text ? JSON.parse(text) : null;
  } catch {
    json = { raw: text.slice(0, 500) };
  }
  if (res.status === 401 || res.status === 403) {
    fail(EXIT.auth, `HTTP ${res.status} auth failed (check CURSOR_API). ${redact(text.slice(0, 200))}`);
  }
  if (res.status === 409) {
    fail(EXIT.busy, `HTTP 409 busy/conflict: ${redact(text.slice(0, 400))}`);
  }
  if (!res.ok) {
    fail(EXIT.api, `HTTP ${res.status}: ${redact(text.slice(0, 600))}`);
  }
  return { status: res.status, json, text };
}

// ─── agents helpers ─────────────────────────────────────────────────────────

type AgentItem = {
  id?: string;
  name?: string;
  status?: string;
  url?: string;
  createdAt?: string;
  updatedAt?: string;
  latestRunId?: string;
  env?: { type?: string; name?: string };
  repos?: unknown[];
};

function asItems(json: unknown): AgentItem[] {
  if (!json || typeof json !== "object") return [];
  const o = json as Record<string, unknown>;
  const items = o.items ?? o.agents ?? o.data;
  return Array.isArray(items) ? (items as AgentItem[]) : [];
}

async function listAllAgents(opts: {
  limit: number;
  all: boolean;
  includeArchived: boolean;
}): Promise<AgentItem[]> {
  const out: AgentItem[] = [];
  let cursor: string | undefined;
  const pageLimit = Math.min(Math.max(opts.limit, 1), 100);
  for (let page = 0; page < (opts.all ? 20 : 1); page++) {
    const qs = new URLSearchParams();
    qs.set("limit", String(pageLimit));
    if (!opts.includeArchived) qs.set("includeArchived", "false");
    if (cursor) qs.set("cursor", cursor);
    const { json } = await api("GET", `/agents?${qs}`);
    const batch = asItems(json);
    out.push(...batch);
    const next =
      json && typeof json === "object"
        ? (json as { nextCursor?: string }).nextCursor
        : undefined;
    if (!opts.all || !next || batch.length === 0) break;
    cursor = next;
  }
  return out;
}

function statusRank(status: string): number {
  const s = (status || "").toUpperCase();
  const order: Record<string, number> = {
    RUNNING: 0,
    ACTIVE: 0,
    IDLE: 1,
    WAITING_FOR_BACKGROUND_WORK: 2,
    NOT_YET_STARTED: 3,
    ERROR: 4,
    EXPIRED: 8,
    ARCHIVED: 9,
  };
  return order[s] ?? 5;
}

async function resolveAgentId(label: string): Promise<string> {
  if (/^bc-[\w-]+$/i.test(label)) return label;
  const agents = await listAllAgents({ limit: 100, all: true, includeArchived: true });
  const matches = agents
    .map((a, index) => ({
      a,
      index,
      id: a.id ?? "",
      name: a.name ?? "",
      status: a.status ?? "",
    }))
    .filter((row) => row.name === label || row.id === label);
  if (matches.length === 0) {
    fail(EXIT.usage, `cannot resolve agent '${label}' (try: cloud.ts list --all)`);
  }
  matches.sort((x, y) => statusRank(x.status) - statusRank(y.status) || x.index - y.index);
  const id = matches[0]!.id;
  if (!id.startsWith("bc-")) fail(EXIT.api, `resolved id looks invalid for '${label}'`);
  return id;
}

function printTable(agents: AgentItem[]): void {
  const nameW = 22;
  const idW = 40;
  const stW = 10;
  console.log(
    `${"name".padEnd(nameW)} ${"id".padEnd(idW)} ${"status".padEnd(stW)} env                  latestRun`,
  );
  console.log("-".repeat(120));
  for (const a of agents) {
    const name = (a.name ?? "").slice(0, nameW).padEnd(nameW);
    const id = (a.id ?? "").padEnd(idW);
    const st = (a.status ?? "").padEnd(stW);
    const env = (a.env?.name ?? a.env?.type ?? "-").slice(0, 20).padEnd(20);
    const lr = (a.latestRunId ?? "").slice(0, 16);
    console.log(`${name} ${id} ${st} ${env} ${lr}`);
  }
}

function printJson(value: unknown): void {
  console.log(JSON.stringify(value, null, 2));
}

// ─── commands ───────────────────────────────────────────────────────────────

function flag(args: string[], name: string): boolean {
  return args.includes(name);
}

function opt(args: string[], name: string): string | undefined {
  const i = args.indexOf(name);
  if (i < 0) return undefined;
  return args[i + 1];
}

function requireOpt(args: string[], name: string): string {
  const v = opt(args, name);
  if (!v || v.startsWith("-")) usage(`${name} requires a value`);
  return v;
}

async function cmdMe(jsonOut: boolean): Promise<void> {
  const { source } = resolveApiKey();
  const { json } = await api("GET", "/me");
  if (jsonOut) {
    printJson({ authSource: source, me: json });
    return;
  }
  const o = (json ?? {}) as Record<string, unknown>;
  console.log(`authSource: ${source}`);
  console.log(`apiKeyName: ${o.apiKeyName ?? "?"}`);
  if (o.userEmail) console.log(`userEmail:  ${o.userEmail}`);
  if (o.userId !== undefined) console.log(`userId:     ${o.userId}`);
  if (o.createdAt) console.log(`createdAt:  ${o.createdAt}`);
}

async function cmdList(args: string[]): Promise<void> {
  const jsonOut = flag(args, "--json");
  const activeOnly = flag(args, "--active");
  const archivedOnly = flag(args, "--archived");
  const all = flag(args, "--all");
  const envFilter = opt(args, "--env");
  const limit = Number(opt(args, "--limit") ?? (all ? 100 : 50));
  let agents = await listAllAgents({
    limit,
    all,
    includeArchived: !activeOnly || archivedOnly,
  });
  if (activeOnly) agents = agents.filter((a) => (a.status ?? "").toUpperCase() === "ACTIVE");
  if (archivedOnly) agents = agents.filter((a) => (a.status ?? "").toUpperCase() === "ARCHIVED");
  if (envFilter) {
    agents = agents.filter(
      (a) => a.env?.name === envFilter || (a.env?.name ?? "").includes(envFilter),
    );
  }
  if (jsonOut) {
    printJson({ count: agents.length, items: agents });
    return;
  }
  console.log(`count=${agents.length}${all ? " (paged)" : ""}`);
  printTable(agents);
}

async function cmdGet(args: string[]): Promise<void> {
  const label = args.find((a) => !a.startsWith("-"));
  if (!label) usage("get requires <bcId|name>");
  const id = await resolveAgentId(label);
  const { json } = await api("GET", `/agents/${id}`);
  if (flag(args, "--json")) {
    printJson(json);
    return;
  }
  const a = (json && typeof json === "object" && "agent" in (json as object)
    ? (json as { agent: AgentItem }).agent
    : json) as AgentItem;
  console.log(`id:      ${a.id ?? id}`);
  console.log(`name:    ${a.name ?? "?"}`);
  console.log(`status:  ${a.status ?? "?"}`);
  console.log(`url:     ${a.url ?? `https://cursor.com/agents/${id}`}`);
  console.log(`env:     ${a.env?.name ?? a.env?.type ?? "-"}`);
  console.log(`latest:  ${a.latestRunId ?? "-"}`);
}

async function cmdCreate(args: string[]): Promise<void> {
  const prompt = requireOpt(args, "--prompt");
  const name = opt(args, "--name");
  const envName = opt(args, "--env");
  const repo = opt(args, "--repo");
  const ref = opt(args, "--ref") ?? "main";
  const model = opt(args, "--model");
  const autoPr = flag(args, "--auto-pr");

  const body: Record<string, unknown> = {
    prompt: { text: prompt },
    autoCreatePR: autoPr,
  };
  if (name) body.name = name;
  if (model) body.model = { id: model };
  if (envName) {
    body.env = { type: "cloud", name: envName };
  } else if (repo) {
    body.repos = [{ url: repo, startingRef: ref }];
  } else {
    // default agenterm cloud env used by fleet skills
    body.env = { type: "cloud", name: "mgttt/agenterm" };
  }

  const { json } = await api("POST", "/agents", body);
  if (flag(args, "--json")) {
    printJson(json);
    return;
  }
  const root = json as { agent?: AgentItem; run?: { id?: string; status?: string } };
  const agent = root.agent ?? (json as AgentItem);
  console.log(`created name=${agent.name ?? name ?? "?"} id=${agent.id ?? "?"}`);
  console.log(`url=${agent.url ?? (agent.id ? `https://cursor.com/agents/${agent.id}` : "?")}`);
  if (root.run) console.log(`run=${root.run.id ?? "?"} status=${root.run.status ?? "?"}`);
}

async function cmdChat(args: string[]): Promise<void> {
  const to = requireOpt(args, "--to");
  const prompt = requireOpt(args, "--prompt");
  const from = opt(args, "--from") ?? "local";
  const id = await resolveAgentId(to);
  const text = `<from::${from}><to::${to}>\n${prompt}`;
  const { json } = await api("POST", `/agents/${id}/runs`, { prompt: { text } });
  if (flag(args, "--json")) {
    printJson(json);
    return;
  }
  const run =
    json && typeof json === "object" && "run" in (json as object)
      ? (json as { run: { id?: string; status?: string } }).run
      : (json as { id?: string; status?: string });
  console.log(`chat ok agent=${id} run=${run?.id ?? "?"} status=${run?.status ?? "?"}`);
}

async function cmdRuns(args: string[]): Promise<void> {
  const label = args.find((a) => !a.startsWith("-") && a !== "runs");
  if (!label) usage("runs requires <bcId|name>");
  const id = await resolveAgentId(label);
  const limit = opt(args, "--limit") ?? "20";
  const { json } = await api("GET", `/agents/${id}/runs?limit=${limit}`);
  if (flag(args, "--json")) {
    printJson(json);
    return;
  }
  const items =
    json && typeof json === "object" && Array.isArray((json as { items?: unknown[] }).items)
      ? ((json as { items: Array<Record<string, unknown>> }).items ?? [])
      : [];
  console.log(`runs for ${id} count=${items.length}`);
  for (const r of items) {
    console.log(
      `${String(r.id ?? "?").padEnd(42)} ${(r.status ?? "?").toString().padEnd(12)} ${r.createdAt ?? ""}`,
    );
  }
}

async function cmdRunGet(args: string[]): Promise<void> {
  const positional = args.filter((a) => !a.startsWith("-"));
  // positional may be ["run-get", agent, runId] or [agent, runId]
  const cleaned = positional[0] === "run-get" ? positional.slice(1) : positional;
  const [label, runId] = cleaned;
  if (!label || !runId) usage("run-get requires <bcId|name> <runId>");
  const id = await resolveAgentId(label);
  const { json } = await api("GET", `/agents/${id}/runs/${runId}`);
  if (flag(args, "--json")) {
    printJson(json);
    return;
  }
  const r =
    json && typeof json === "object" && "run" in (json as object)
      ? (json as { run: Record<string, unknown> }).run
      : (json as Record<string, unknown>);
  console.log(`run:      ${r.id ?? runId}`);
  console.log(`status:   ${r.status ?? "?"}`);
  console.log(`duration: ${r.durationMs ?? "-"} ms`);
  if (typeof r.result === "string" && r.result) {
    const snippet = r.result.length > 400 ? `${r.result.slice(0, 400)}…` : r.result;
    console.log(`result:\n${snippet}`);
  }
}

async function cmdCancel(args: string[]): Promise<void> {
  const positional = args.filter((a) => !a.startsWith("-"));
  const cleaned = positional[0] === "cancel" ? positional.slice(1) : positional;
  const [label, maybeRun] = cleaned;
  if (!label) usage("cancel requires <bcId|name> [runId]");
  const id = await resolveAgentId(label);
  let runId = maybeRun;
  if (!runId) {
    const { json } = await api("GET", `/agents/${id}`);
    const a = (json && typeof json === "object" && "latestRunId" in (json as object)
      ? json
      : (json as { agent?: AgentItem }).agent) as AgentItem;
    runId = a.latestRunId;
  }
  if (!runId) fail(EXIT.usage, "no runId provided and latestRunId missing");
  const { json } = await api("POST", `/agents/${id}/runs/${runId}/cancel`);
  if (flag(args, "--json")) printJson(json);
  else console.log(`cancelled run=${runId} agent=${id}`);
}

async function cmdArchive(args: string[], unarchive: boolean): Promise<void> {
  const label = args.find((a) => !a.startsWith("-") && a !== "archive" && a !== "unarchive");
  if (!label) usage(`${unarchive ? "unarchive" : "archive"} requires <bcId|name>`);
  const id = await resolveAgentId(label);
  const path = unarchive ? `/agents/${id}/unarchive` : `/agents/${id}/archive`;
  const { json } = await api("POST", path);
  if (flag(args, "--json")) printJson(json);
  else console.log(`${unarchive ? "unarchived" : "archived"} ${id}`);
}

async function cmdModels(args: string[]): Promise<void> {
  const { json } = await api("GET", "/models");
  if (flag(args, "--json")) {
    printJson(json);
    return;
  }
  const items =
    json && typeof json === "object" && Array.isArray((json as { items?: unknown[] }).items)
      ? ((json as { items: Array<{ id?: string; displayName?: string }> }).items ?? [])
      : [];
  for (const m of items) {
    console.log(`${(m.id ?? "?").padEnd(36)} ${m.displayName ?? ""}`);
  }
}

// ─── main ───────────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  const argv = process.argv.slice(2);
  if (argv.length === 0 || flag(argv, "-h") || flag(argv, "--help")) usage();

  const cmd = argv[0]!;
  const rest = argv.slice(1);

  switch (cmd) {
    case "me":
    case "auth-check":
      await cmdMe(flag(rest, "--json") || flag(argv, "--json"));
      break;
    case "list":
    case "ls":
      await cmdList(rest);
      break;
    case "get":
      await cmdGet(rest);
      break;
    case "create":
    case "spawn":
      await cmdCreate(rest);
      break;
    case "chat":
    case "prompt":
      await cmdChat(rest);
      break;
    case "runs":
      await cmdRuns(rest);
      break;
    case "run-get":
      await cmdRunGet(rest);
      break;
    case "cancel":
      await cmdCancel(rest);
      break;
    case "archive":
      await cmdArchive(rest, false);
      break;
    case "unarchive":
      await cmdArchive(rest, true);
      break;
    case "models":
      await cmdModels(rest);
      break;
    default:
      usage(`unknown command: ${cmd}`);
  }
}

main().catch((err) => {
  fail(EXIT.api, err instanceof Error ? err.message : String(err));
});
