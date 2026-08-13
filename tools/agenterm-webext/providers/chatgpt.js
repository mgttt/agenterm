import {
  bodyText,
  findLimitReachedBanner,
  findPlanName,
  parseResetDate,
  remainingFromUsedLimit,
  deepSearch,
  textOf,
} from "../lib/extract-helpers.js";

const PLAN_CANDIDATES = [
  "Business",
  "Team",
  "Enterprise",
  "Pro",
  "Plus",
  "Free",
];

/**
 * ChatGPT / OpenAI usage extractor.
 *
 * Pages: chatgpt.com settings, chat.openai.com, platform.openai.com usage/settings.
 * Selectors are best-effort — tune after a live pass on account settings / usage UI.
 */
export const chatgptProvider = {
  id: "chatgpt",
  name: "ChatGPT / OpenAI",
  matchUrls: [
    "https://chatgpt.com/*",
    "https://chat.openai.com/*",
    "https://platform.openai.com/account/usage",
    "https://platform.openai.com/usage",
    "https://platform.openai.com/settings/*",
    "https://platform.openai.com/account/billing*",
  ],

  extractFromDom(document) {
    const text = bodyText(document);
    const limitsReached = findLimitReachedBanner(text) || findRateLimitBanner(text);

    const plan =
      extractExplicitPlan(text) ??
      extractPlanFromTitle(text) ??
      extractPlanFromDom(document) ??
      findPlanName(text, PLAN_CANDIDATES);

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

    const usedBeforeMatch = text.match(/used\s+(\d+(?:\.\d+)?)\s*%/i);
    if (usedBeforeMatch) {
      used = Number(usedBeforeMatch[1]);
      if (remainingPct == null) remainingPct = Math.max(0, 100 - used);
    }

    const ofMatch = text.match(
      /(\d+)\s*of\s*(\d+)\s*(?:messages?|requests?|credits?|chats?)/i
    );
    if (ofMatch) {
      used = Number(ofMatch[1]);
      limit = Number(ofMatch[2]);
      remainingPct = remainingFromUsedLimit(used, limit);
    }

    const leftMatch = text.match(
      /(\d+)\s*(?:messages?|requests?|chats?)\s*(?:left|remaining)/i
    );
    if (leftMatch && limit == null) {
      const left = Number(leftMatch[1]);
      const totalMatch = text.match(
        /(\d+)\s*(?:messages?|requests?|chats?)\s*(?:per|\/)\s*(?:day|week|month)/i
      );
      if (totalMatch) {
        limit = Number(totalMatch[1]);
        used = Math.max(0, limit - left);
        remainingPct = remainingFromUsedLimit(used, limit);
      }
    }

    const pctNearLabel = text.match(
      /(?:remaining|left|quota|usage)[^%\n]{0,40}(\d+(?:\.\d+)?)\s*%/i
    );
    if (pctNearLabel && remainingPct == null) {
      remainingPct = Number(pctNearLabel[1]);
    }

    const progress = document.querySelector(
      "[role='progressbar'][aria-valuenow], progress[value]"
    );
    if (progress) {
      const now =
        progress.getAttribute("aria-valuenow") ?? progress.getAttribute("value");
      if (now != null) {
        used = Number(now);
        remainingPct = Math.max(0, Math.round(100 - used));
      }
    }

    const resetAt =
      parseResetDate(text) ??
      extractResetFromDom(document) ??
      parseRelativeReset(text);

    if (limitsReached) remainingPct = 0;

    if (!plan && remainingPct == null && !limitsReached && used == null) {
      return null;
    }

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
    const hits = deepSearch(
      json,
      "(usage|quota|limit|remaining|plan|subscription|rate|billing|tier)"
    );
    let plan = null;
    let remainingPct = null;
    let limitsReached = false;
    let resetAt = null;
    let used = null;
    let limit = null;

    for (const { key, value } of hits) {
      if (/plan|tier|subscription|account_type/i.test(key) && typeof value === "string") {
        plan = normalizePlan(value) ?? value;
      }
      if (/remaining/i.test(key) && typeof value === "number") {
        remainingPct = value <= 1 ? Math.round(value * 100) : value;
      }
      if (/used/i.test(key) && typeof value === "number" && /usage|quota/i.test(key)) {
        used = value;
      }
      if (/limit|cap|max/i.test(key) && typeof value === "number") {
        limit = value;
      }
      if (/rate.?limit|exhausted|limit.?reached/i.test(key) && value) {
        limitsReached = true;
      }
      if (/reset/i.test(key) && typeof value === "string") {
        resetAt = value;
      }
    }

    if (used != null && limit != null && remainingPct == null) {
      remainingPct = remainingFromUsedLimit(used, limit);
    }

    if (!plan && !remainingPct && !limitsReached && used == null) return null;

    return buildSnapshot({
      used,
      limit,
      remainingPct: limitsReached ? 0 : remainingPct,
      resetAt,
      plan,
      limitsReached,
      raw: { jsonHits: hits.slice(0, 20) },
    });
  },
};

function findRateLimitBanner(text) {
  return /rate\s+limit|too\s+many\s+requests|try\s+again\s+later|message\s+limit/i.test(
    text
  );
}

function extractExplicitPlan(text) {
  const labeled = text.match(/plan[:\s]+(Free|Plus|Pro|Team|Business|Enterprise)/i);
  if (labeled) return labeled[1];
  const forPlan = text.match(/for\s+(Free|Plus|Pro|Team|Business|Enterprise)\s+plan/i);
  if (forPlan) return forPlan[1];
  return null;
}

function extractPlanFromTitle(text) {
  const m = text.match(/ChatGPT\s+(Plus|Pro|Team|Business|Enterprise|Free)/i);
  return m ? m[1] : null;
}

function extractPlanFromDom(document) {
  for (const el of document.querySelectorAll(
    "[data-testid*='plan'], [class*='plan'], [class*='subscription'], [class*='tier'], h1, h2, h3"
  )) {
    const t = textOf(el);
    const branded = t.match(/ChatGPT\s+(Plus|Pro|Team|Business|Enterprise|Free)/i);
    if (branded) return branded[1];
    const plain = t.match(/\b(Business|Team|Enterprise|Pro|Plus|Free)\b/i);
    if (plain && /plan|subscription|tier|account/i.test(t)) return plain[1];
  }
  return null;
}

function extractResetFromDom(document) {
  for (const el of document.querySelectorAll("time[datetime]")) {
    const label = textOf(el.parentElement);
    if (/reset|billing|period|renews?/i.test(label)) {
      return el.getAttribute("datetime") ?? textOf(el);
    }
  }
  return null;
}

function parseRelativeReset(text) {
  const m = text.match(
    /(?:resets?|renews?)\s+(?:in\s+)?(\d+)\s*(hour|minute|day)s?/i
  );
  if (m) return `in ${m[1]} ${m[2]}${Number(m[1]) === 1 ? "" : "s"}`;
  return null;
}

function normalizePlan(value) {
  const m = String(value).match(
    /\b(Business|Team|Enterprise|Pro|Plus|Free)\b/i
  );
  return m ? m[1] : null;
}

function buildSnapshot(fields) {
  return {
    providerId: "chatgpt",
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
