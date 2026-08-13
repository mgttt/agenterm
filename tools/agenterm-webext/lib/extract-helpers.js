/**
 * Shared DOM/text parsing helpers for provider extractors.
 */

export function textOf(el) {
  return (el?.textContent ?? "").replace(/\s+/g, " ").trim();
}

export function bodyText(doc) {
  const body = doc.body;
  if (!body) return "";
  const text = body.innerText?.trim() ? body.innerText : body.textContent ?? "";
  return text.replace(/\r/g, "");
}

export function findLimitReachedBanner(text) {
  const patterns = [
    /usage\s+limits?\s+reached/i,
    /limit\s+reached/i,
    /quota\s+exhausted/i,
    /out\s+of\s+(usage|credits|requests)/i,
    /no\s+remaining\s+(usage|credits)/i,
  ];
  return patterns.some((p) => p.test(text));
}

export function parsePercent(text) {
  const m = text.match(/(\d+(?:\.\d+)?)\s*%/);
  return m ? Number(m[1]) : null;
}

export function parseMoney(text) {
  const m = text.match(/\$\s*([\d,]+(?:\.\d+)?)/);
  return m ? Number(m[1].replace(/,/g, "")) : null;
}

export function parseResetDate(text) {
  const patterns = [
    /resets?\s+(?:on\s+)?([A-Za-z]+\s+\d{1,2},?\s+\d{4})/i,
    /resets?\s+(?:on\s+)?(\d{4}-\d{2}-\d{2})/i,
    /next\s+reset[:\s]+([A-Za-z]+\s+\d{1,2},?\s+\d{4})/i,
    /billing\s+period\s+ends?\s+([A-Za-z]+\s+\d{1,2},?\s+\d{4})/i,
  ];
  for (const p of patterns) {
    const m = text.match(p);
    if (m) return m[1];
  }
  return null;
}

export function findPlanName(text, candidates) {
  for (const name of candidates) {
    if (text.toLowerCase().includes(name.toLowerCase())) return name;
  }
  const m = text.match(/plan[:\s]+([A-Za-z0-9+ -]+)/i);
  return m ? m[1].trim() : null;
}

export function remainingFromUsedLimit(used, limit) {
  if (used == null || limit == null || limit <= 0) return null;
  const remaining = limit - used;
  return Math.max(0, Math.round((remaining / limit) * 100));
}

export function tryParseEmbeddedJson(doc) {
  const results = [];
  for (const script of doc.querySelectorAll("script[type='application/json'], script#__NEXT_DATA__")) {
    try {
      const data = JSON.parse(script.textContent);
      results.push(data);
    } catch {
      // skip
    }
  }
  return results;
}

export function deepSearch(obj, keyPattern, maxDepth = 8) {
  const hits = [];
  const re = new RegExp(keyPattern, "i");
  function walk(node, depth) {
    if (depth > maxDepth || node == null) return;
    if (typeof node !== "object") return;
    if (Array.isArray(node)) {
      for (const item of node) walk(item, depth + 1);
      return;
    }
    for (const [k, v] of Object.entries(node)) {
      if (re.test(k)) hits.push({ key: k, value: v });
      walk(v, depth + 1);
    }
  }
  walk(obj, 0);
  return hits;
}
