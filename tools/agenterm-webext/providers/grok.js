import {
  bodyText,
  findLimitReachedBanner,
  findPlanName,
  parseResetDate,
  remainingFromUsedLimit,
  deepSearch,
  textOf,
} from "../lib/extract-helpers.js";

/**
 * Grok / xAI usage extractor.
 *
 * Pages: grok.com, accounts.x.ai (subscription / usage if shown).
 * Tune selectors after inspecting live SuperGrok billing UI.
 */
export const grokProvider = {
  id: "grok",
  name: "Grok / xAI",
  matchUrls: [
    "https://grok.com/*",
    "https://accounts.x.ai/*",
  ],

  extractFromDom(document) {
    const text = bodyText(document);
    const limitsReached = findLimitReachedBanner(text);

    const plan =
      findPlanName(text, [
        "SuperGrok",
        "Super Grok",
        "Grok Pro",
        "Premium",
        "Free",
      ]) ?? extractPlanFromDom(document);

    let remainingPct = null;
    let used = null;
    let limit = null;

    const remainingMatch = text.match(/(\d+(?:\.\d+)?)\s*%\s*remaining/i);
    if (remainingMatch) {
      remainingPct = Number(remainingMatch[1]);
    }

    const usedMatch = text.match(/(\d+(?:\.\d+)?)\s*%\s*used/i);
    if (usedMatch) {
      used = Number(usedMatch[1]);
      if (remainingPct == null) remainingPct = Math.max(0, 100 - used);
    }

    // "X of Y" requests/credits
    const ofMatch = text.match(/(\d+)\s*of\s*(\d+)\s*(?:requests?|credits?|messages?)/i);
    if (ofMatch) {
      used = Number(ofMatch[1]);
      limit = Number(ofMatch[2]);
      remainingPct = remainingFromUsedLimit(used, limit);
    }

  const pctNearLabel = text.match(
      /(?:remaining|left|quota)[^%\n]{0,40}(\d+(?:\.\d+)?)\s*%/i
    );
    if (pctNearLabel && remainingPct == null) {
      remainingPct = Number(pctNearLabel[1]);
    }

    const resetAt = parseResetDate(text);

    if (limitsReached) remainingPct = 0;

    return buildSnapshot({
      used,
      limit,
      remainingPct,
      resetAt,
      plan,
      limitsReached,
      raw: {
        url: document.location?.href,
        snippet: text.slice(0, 500),
      },
    });
  },

  extractFromJson(json) {
    if (!json) return null;
    const hits = deepSearch(json, "(usage|quota|remaining|plan|subscription|credits)");
    let plan = null;
    let remainingPct = null;
    let limitsReached = false;

    for (const { key, value } of hits) {
      if (/plan|tier|subscription/i.test(key) && typeof value === "string") {
        plan = value;
      }
      if (/remaining/i.test(key) && typeof value === "number") {
        remainingPct = value <= 1 ? Math.round(value * 100) : value;
      }
      if (/exhausted|limit/i.test(key) && value === true) limitsReached = true;
    }

    if (!plan && !remainingPct && !limitsReached) return null;

    return buildSnapshot({
      used: null,
      limit: null,
      remainingPct: limitsReached ? 0 : remainingPct,
      resetAt: null,
      plan,
      limitsReached,
      raw: { jsonHits: hits.slice(0, 20) },
    });
  },
};

function extractPlanFromDom(document) {
  for (const el of document.querySelectorAll(
    "[class*='subscription'], [class*='plan'], [data-tier], h1, h2"
  )) {
    const t = textOf(el);
    const m = t.match(/\b(SuperGrok|Super Grok|Grok Pro|Premium|Free)\b/i);
    if (m) return m[1];
  }
  return null;
}

function buildSnapshot(fields) {
  return {
    providerId: "grok",
    used: fields.used,
    limit: fields.limit,
    remainingPct: fields.remainingPct,
    resetAt: fields.resetAt,
    plan: fields.plan,
    limitsReached: fields.limitsReached,
    raw: fields.raw,
    capturedAt: Date.now(),
  };
}
