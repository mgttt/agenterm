/**
 * Unit tests for provider extractors (Node.js, no Chrome APIs).
 * Run: node tests/extractors.test.js
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { JSDOM } from "jsdom";

import { cursorProvider } from "../providers/cursor.js";
import { grokProvider } from "../providers/grok.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixtures = join(__dirname, "..", "fixtures");

function loadFixture(name) {
  const html = readFileSync(join(fixtures, name), "utf8");
  return new JSDOM(html, { url: "https://cursor.com/dashboard/usage" }).window.document;
}

function assert(cond, msg) {
  if (!cond) {
    console.error("FAIL:", msg);
    process.exitCode = 1;
    throw new Error(msg);
  }
  console.log("PASS:", msg);
}

// Cursor usage page
const cursorDoc = loadFixture("cursor-usage.html");
const cursorSnap = cursorProvider.extractFromDom(cursorDoc);
assert(cursorSnap.plan === "Ultra", "cursor plan Ultra");
assert(cursorSnap.used === 87, "cursor used 87%");
assert(cursorSnap.remainingPct === 13, "cursor remaining 13%");
assert(cursorSnap.resetAt?.includes("March"), "cursor reset date");
assert(cursorSnap.raw.onDemandSpend === 12.5, "cursor on-demand spend");

// Cursor agents limit banner
const agentsDoc = new JSDOM(
  readFileSync(join(fixtures, "cursor-agents-limit.html"), "utf8"),
  { url: "https://cursor.com/agents" }
).window.document;
const agentsSnap = cursorProvider.extractFromDom(agentsDoc);
assert(agentsSnap.limitsReached === true, "cursor agents limit banner");
assert(agentsSnap.remainingPct === 0, "cursor agents remaining 0");

// Grok usage
const grokDoc = new JSDOM(
  readFileSync(join(fixtures, "grok-usage.html"), "utf8"),
  { url: "https://grok.com" }
).window.document;
const grokSnap = grokProvider.extractFromDom(grokDoc);
assert(grokSnap.plan?.toLowerCase().includes("supergrok"), "grok plan SuperGrok");
assert(grokSnap.remainingPct === 42, "grok 42% remaining");

// Grok "X of Y" pattern
const grokOfDoc = new JSDOM(
  `<html><body><p>SuperGrok</p><p>150 of 500 requests used</p></body></html>`,
  { url: "https://grok.com" }
).window.document;
const grokOfSnap = grokProvider.extractFromDom(grokOfDoc);
assert(grokOfSnap.used === 150 && grokOfSnap.limit === 500, "grok 150/500");
assert(grokOfSnap.remainingPct === 70, "grok 70% remaining from of");

console.log("\nDone. exitCode=", process.exitCode ?? 0);
