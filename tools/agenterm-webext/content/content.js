/**
 * Content script: runs on matched provider pages, extracts usage, sends to background.
 */
(function () {
  const href = window.location.href;

  function detectProvider() {
    if (href.startsWith("https://cursor.com/dashboard") || href.startsWith("https://cursor.com/agents")) {
      return extractCursor();
    }
    if (href.startsWith("https://grok.com") || href.startsWith("https://accounts.x.ai")) {
      return extractGrok();
    }
    if (
      href.startsWith("https://chatgpt.com") ||
      href.startsWith("https://chat.openai.com") ||
      href.startsWith("https://platform.openai.com")
    ) {
      return extractChatgpt();
    }
    return null;
  }

  function extractCursor() {
    const text = document.body?.innerText ?? "";
    const limitsReached = /usage\s+limits?\s+reached|limit\s+reached/i.test(text);
    let plan = null;
    const planMatch = text.match(/\b(Ultra|Pro|Business|Hobby|Free)\b/i);
    if (planMatch) plan = planMatch[1];

    let used = null;
    let remainingPct = null;
    const included = text.match(/included[^%\n]{0,80}?(\d+(?:\.\d+)?)\s*%/i);
    if (included) {
      used = Number(included[1]);
      remainingPct = Math.max(0, 100 - used);
    }
    const usedPct = text.match(/(\d+(?:\.\d+)?)\s*%\s*used/i);
    if (usedPct) {
      used = Number(usedPct[1]);
      remainingPct = Math.max(0, 100 - used);
    }
    const usedBefore = text.match(/used\s+(\d+(?:\.\d+)?)\s*%/i);
    if (usedBefore) {
      used = Number(usedBefore[1]);
      remainingPct = Math.max(0, 100 - used);
    }
    const rem = text.match(/(\d+(?:\.\d+)?)\s*%\s*remaining/i);
    if (rem) remainingPct = Number(rem[1]);

    const progress = document.querySelector("[role='progressbar'][aria-valuenow]");
    if (progress) {
      used = Number(progress.getAttribute("aria-valuenow"));
      remainingPct = Math.max(0, 100 - used);
    }

    let onDemandSpend = null;
    const spend = text.match(/on[- ]?demand[^$]{0,40}\$\s*([\d,]+(?:\.\d+)?)/i);
    if (spend) onDemandSpend = Number(spend[1].replace(/,/g, ""));

    let resetAt = null;
    const reset = text.match(/resets?\s+(?:on\s+)?([A-Za-z]+\s+\d{1,2},?\s+\d{4})/i);
    if (reset) resetAt = reset[1];

    if (limitsReached) remainingPct = 0;

    return {
      providerId: "cursor",
      used,
      limit: used != null ? 100 : null,
      remainingPct,
      resetAt,
      plan,
      limitsReached,
      raw: { url: href, onDemandSpend, snippet: text.slice(0, 400) },
      capturedAt: Date.now(),
    };
  }

  function extractGrok() {
    const text = document.body?.innerText ?? "";
    const limitsReached = /usage\s+limits?\s+reached|limit\s+reached|quota\s+exhausted/i.test(text);
    let plan = null;
    const p = text.match(/\b(SuperGrok|Super Grok|Grok Pro|Premium|Free)\b/i);
    if (p) plan = p[1];

    let used = null;
    let limit = null;
    let remainingPct = null;
    const rem = text.match(/(\d+(?:\.\d+)?)\s*%\s*remaining/i);
    if (rem) remainingPct = Number(rem[1]);
    const of = text.match(/(\d+)\s*of\s*(\d+)\s*(?:requests?|credits?)/i);
    if (of) {
      used = Number(of[1]);
      limit = Number(of[2]);
      remainingPct = Math.max(0, Math.round(((limit - used) / limit) * 100));
    }
    if (limitsReached) remainingPct = 0;

    return {
      providerId: "grok",
      used,
      limit,
      remainingPct,
      resetAt: null,
      plan,
      limitsReached,
      raw: { url: href, snippet: text.slice(0, 400) },
      capturedAt: Date.now(),
    };
  }

  function extractChatgpt() {
    const text = document.body?.innerText ?? "";
    const limitsReached =
      /usage\s+limits?\s+reached|limit\s+reached|rate\s+limit|too\s+many\s+requests|message\s+limit/i.test(
        text
      );

    let plan = null;
    const branded = text.match(/ChatGPT\s+(Plus|Pro|Team|Business|Enterprise|Free)/i);
    if (branded) {
      plan = branded[1];
    } else {
      const plain = text.match(/\b(Business|Team|Enterprise|Pro|Plus|Free)\b/i);
      if (plain) plan = plain[1];
    }

    let used = null;
    let limit = null;
    let remainingPct = null;

    const rem = text.match(/(\d+(?:\.\d+)?)\s*%\s*remaining/i);
    if (rem) remainingPct = Number(rem[1]);

    const usedPct = text.match(/(\d+(?:\.\d+)?)\s*%\s*used/i);
    if (usedPct) {
      used = Number(usedPct[1]);
      remainingPct = Math.max(0, 100 - used);
    }
    const usedBefore = text.match(/used\s+(\d+(?:\.\d+)?)\s*%/i);
    if (usedBefore) {
      used = Number(usedBefore[1]);
      remainingPct = Math.max(0, 100 - used);
    }

    const of = text.match(/(\d+)\s*of\s*(\d+)\s*(?:messages?|requests?|chats?)/i);
    if (of) {
      used = Number(of[1]);
      limit = Number(of[2]);
      remainingPct = Math.max(0, Math.round(((limit - used) / limit) * 100));
    }

    const progress = document.querySelector("[role='progressbar'][aria-valuenow]");
    if (progress) {
      used = Number(progress.getAttribute("aria-valuenow"));
      remainingPct = Math.max(0, 100 - used);
    }

    let resetAt = null;
    const reset = text.match(/resets?\s+(?:on\s+)?([A-Za-z]+\s+\d{1,2},?\s+\d{4})/i);
    if (reset) resetAt = reset[1];
    const billing = text.match(/billing\s+period\s+ends?\s+([A-Za-z]+\s+\d{1,2},?\s+\d{4})/i);
    if (billing) resetAt = billing[1];

    if (limitsReached) remainingPct = 0;

    return {
      providerId: "chatgpt",
      used,
      limit,
      remainingPct,
      resetAt,
      plan,
      limitsReached,
      raw: { url: href, snippet: text.slice(0, 400) },
      capturedAt: Date.now(),
    };
  }

  function send() {
    const snapshot = detectProvider();
    if (!snapshot) return;
    chrome.runtime.sendMessage({ type: "snapshot", snapshot });
  }

  send();
  const observer = new MutationObserver(() => send());
  observer.observe(document.body, { childList: true, subtree: true });
  setTimeout(() => observer.disconnect(), 60000);
})();
