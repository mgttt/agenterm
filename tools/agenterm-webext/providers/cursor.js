import {
  bodyText,
  findLimitReachedBanner,
  findPlanName,
  parseResetDate,
  parseMoney,
  deepSearch,
  textOf,
} from "../lib/extract-helpers.js";

/**
 * Cursor usage extractor (Ultra / Pro / etc.)
 *
 * Pages: dashboard, usage, spending, agents (limits banner).
 * Selectors are best-effort — tune after a live pass on cursor.com/dashboard/usage.
 */
export const cursorProvider = {
  id: "cursor",
  name: "Cursor",
  matchUrls: [
    "https://cursor.com/dashboard",
    "https://cursor.com/dashboard/usage",
    "https://cursor.com/dashboard/spending",
    "https://cursor.com/agents",
  ],

  extractFromDom(document) {
    const text = bodyText(document);
    const limitsReached = findLimitReachedBanner(text);

    const plan =
      findPlanName(text, ["Ultra", "Pro", "Business", "Free", "Hobby"]) ??
      extractPlanFromDom(document);

    let includedUsedPct = null;
    let remainingPct = null;
    let used = null;
    let limit = null;
    let onDemandSpend = null;

    // Usage % near "included" / "fast" / "premium" labels
    const includedMatch = text.match(
      /included[^%\n]{0,80}?(\d+(?:\.\d+)?)\s*%/i
    );
    if (includedMatch) {
      includedUsedPct = Number(includedMatch[1]);
      remainingPct = Math.max(0, Math.round(100 - includedUsedPct));
    }

    // "X% used" or "X% remaining"
    const usedPctMatch = text.match(/(\d+(?:\.\d+)?)\s*%\s*used/i);
    if (usedPctMatch) {
      includedUsedPct = Number(usedPctMatch[1]);
      remainingPct = Math.max(0, Math.round(100 - includedUsedPct));
    }
    const usedBeforePct = text.match(/used\s+(\d+(?:\.\d+)?)\s*%/i);
    if (usedBeforePct) {
      includedUsedPct = Number(usedBeforePct[1]);
      remainingPct = Math.max(0, Math.round(100 - includedUsedPct));
    }
    const remainingMatch = text.match(/(\d+(?:\.\d+)?)\s*%\s*remaining/i);
    if (remainingMatch) {
      remainingPct = Number(remainingMatch[1]);
      if (includedUsedPct == null) {
        includedUsedPct = Math.max(0, 100 - remainingPct);
      }
    }

    // Progress bars with aria-valuenow (used portion)
    const progress = document.querySelector(
      "[role='progressbar'][aria-valuenow], progress[value]"
    );
    if (progress) {
      const now =
        progress.getAttribute("aria-valuenow") ?? progress.getAttribute("value");
      if (now != null) {
        includedUsedPct = Number(now);
        remainingPct = Math.max(0, Math.round(100 - includedUsedPct));
      }
    }

    // On-demand spend
    const spendMatch = text.match(/on[- ]?demand[^$]{0,40}\$\s*([\d,]+(?:\.\d+)?)/i);
    if (spendMatch) {
      onDemandSpend = Number(spendMatch[1].replace(/,/g, ""));
    } else {
      onDemandSpend = parseMoney(text.match(/spending/i) ? text : "");
      if (onDemandSpend == null) {
        const spendSection = text.match(/spend(?:ing)?[^$]{0,60}\$\s*([\d,]+(?:\.\d+)?)/i);
        if (spendSection) onDemandSpend = Number(spendSection[1].replace(/,/g, ""));
      }
    }

    const resetAt = parseResetDate(text) ?? extractResetFromDom(document);

    if (limitsReached) {
      remainingPct = 0;
    }

    return buildSnapshot({
      used: includedUsedPct ?? used,
      limit: limit ?? (includedUsedPct != null ? 100 : null),
      remainingPct,
      resetAt,
      plan,
      limitsReached,
      raw: {
        includedUsedPct,
        onDemandSpend,
        url: document.location?.href,
        snippet: text.slice(0, 500),
      },
    });
  },

  extractFromJson(json) {
    if (!json) return null;
    const hits = deepSearch(json, "(usage|quota|limit|remaining|plan|reset)");
    let plan = null;
    let remainingPct = null;
    let limitsReached = false;
    let resetAt = null;

    for (const { key, value } of hits) {
      if (/plan/i.test(key) && typeof value === "string") plan = value;
      if (/remaining/i.test(key) && typeof value === "number") {
        remainingPct = value <= 1 ? Math.round(value * 100) : value;
      }
      if (/limit.*reached|exhausted/i.test(key) && value) limitsReached = true;
      if (/reset/i.test(key) && typeof value === "string") resetAt = value;
    }

    if (!plan && !remainingPct && !limitsReached) return null;

    return buildSnapshot({
      used: null,
      limit: null,
      remainingPct: limitsReached ? 0 : remainingPct,
      resetAt,
      plan,
      limitsReached,
      raw: { jsonHits: hits.slice(0, 20) },
    });
  },
};

function extractPlanFromDom(document) {
  const badges = document.querySelectorAll(
    "[data-plan], [class*='plan'], [class*='badge'], h1, h2"
  );
  for (const el of badges) {
    const t = textOf(el);
    if (/ultra|pro|business|hobby|free/i.test(t)) {
      const m = t.match(/\b(Ultra|Pro|Business|Hobby|Free)\b/i);
      if (m) return m[1];
    }
  }
  return null;
}

function extractResetFromDom(document) {
  const timeEls = document.querySelectorAll("time[datetime]");
  for (const el of timeEls) {
    const label = textOf(el.parentElement);
    if (/reset|billing|period/i.test(label)) {
      return el.getAttribute("datetime") ?? textOf(el);
    }
  }
  return null;
}

function buildSnapshot(fields) {
  return {
    providerId: "cursor",
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
