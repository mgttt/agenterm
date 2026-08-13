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
import { chatgptProvider } from "../providers/chatgpt.js";
import { extractFromDocumentSync } from "../providers/index.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixtures = join(__dirname, "..", "fixtures");

function loadFixture(name, url) {
  const html = readFileSync(join(fixtures, name), "utf8");
  return new JSDOM(html, { url }).window.document;
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
const cursorDoc = loadFixture("cursor-usage.html", "https://cursor.com/dashboard/usage");
const cursorSnap = cursorProvider.extractFromDom(cursorDoc);
assert(cursorSnap.plan === "Ultra", "cursor plan Ultra");
assert(cursorSnap.used === 87, "cursor used 87%");
assert(cursorSnap.remainingPct === 13, "cursor remaining 13%");
assert(cursorSnap.resetAt?.includes("March"), "cursor reset date");
assert(cursorSnap.raw.onDemandSpend === 12.5, "cursor on-demand spend");

// Cursor agents limit banner
const agentsDoc = loadFixture("cursor-agents-limit.html", "https://cursor.com/agents");
const agentsSnap = cursorProvider.extractFromDom(agentsDoc);
assert(agentsSnap.limitsReached === true, "cursor agents limit banner");
assert(agentsSnap.remainingPct === 0, "cursor agents remaining 0");

// Grok usage
const grokDoc = loadFixture("grok-usage.html", "https://grok.com");
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

// ChatGPT Plus settings usage
const chatgptDoc = loadFixture("chatgpt-plus-usage.html", "https://chatgpt.com/settings");
const chatgptSnap = chatgptProvider.extractFromDom(chatgptDoc);
assert(chatgptSnap.plan === "Plus", "chatgpt plan Plus");
assert(chatgptSnap.used === 65, "chatgpt used 65%");
assert(chatgptSnap.remainingPct === 35, "chatgpt remaining 35%");
assert(chatgptSnap.resetAt?.includes("March"), "chatgpt reset date");

// ChatGPT rate limit banner
const chatgptLimitDoc = loadFixture("chatgpt-rate-limit.html", "https://chatgpt.com");
const chatgptLimitSnap = chatgptProvider.extractFromDom(chatgptLimitDoc);
assert(chatgptLimitSnap.limitsReached === true, "chatgpt rate limit banner");
assert(chatgptLimitSnap.remainingPct === 0, "chatgpt limit remaining 0");
assert(chatgptLimitSnap.plan === "Free", "chatgpt free plan on limit page");

// ChatGPT platform usage (DOM + embedded JSON)
const platformDoc = loadFixture(
  "chatgpt-platform-usage.html",
  "https://platform.openai.com/usage"
);
const platformSnap = extractFromDocumentSync(chatgptProvider, platformDoc);
assert(platformSnap.plan?.toLowerCase() === "team", "chatgpt platform team plan");
assert(platformSnap.used === 42 && platformSnap.limit === 100, "chatgpt platform 42/100");
assert(platformSnap.remainingPct === 58, "chatgpt platform 58% remaining");

console.log("\nDone. exitCode=", process.exitCode ?? 0);
