import { getConfig, saveSnapshot, getAlertState, setAlertState } from "./lib/storage.js";
import { sendUsageAlert } from "./lib/notifier.js";
import { providers } from "./providers/index.js";

const ALARM_NAME = "agenterm-webext-poll";
const ALERT_COOLDOWN_MS = 6 * 60 * 60 * 1000; // 6h between repeat alerts per provider

chrome.runtime.onInstalled.addListener(async () => {
  const config = await getConfig();
  await scheduleAlarm(config.pollIntervalMinutes);
});

chrome.runtime.onStartup.addListener(async () => {
  const config = await getConfig();
  await scheduleAlarm(config.pollIntervalMinutes);
});

chrome.alarms.onAlarm.addListener(async (alarm) => {
  if (alarm.name === ALARM_NAME) {
    await pollAllProviders();
  }
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message.type === "snapshot") {
    handleContentSnapshot(message.snapshot).then(() => sendResponse({ ok: true }));
    return true;
  }
  if (message.type === "poll-now") {
    pollAllProviders().then(() => sendResponse({ ok: true }));
    return true;
  }
  if (message.type === "reschedule") {
    getConfig().then((config) => scheduleAlarm(config.pollIntervalMinutes));
    sendResponse({ ok: true });
    return true;
  }
  return false;
});

async function scheduleAlarm(intervalMinutes) {
  const period = Math.max(1, intervalMinutes);
  await chrome.alarms.clear(ALARM_NAME);
  chrome.alarms.create(ALARM_NAME, { periodInMinutes: period });
}

async function pollAllProviders() {
  const config = await getConfig();
  const enabled = providers.filter((p) => config.providers[p.id]?.enabled);

  for (const provider of enabled) {
    await pollProvider(provider);
  }
}

async function pollProvider(provider) {
  const urls = provider.matchUrls.filter((u) => !u.endsWith("*") || u.length > 20);
  const primaryUrl = urls[0] ?? provider.matchUrls[0].replace(/\*$/, "");

  let snapshot = null;

  // Try existing tab first
  const tabs = await chrome.tabs.query({ url: provider.matchUrls.map(toChromePattern) });
  for (const tab of tabs) {
    if (!tab.id) continue;
    try {
      const results = await chrome.scripting.executeScript({
        target: { tabId: tab.id },
        func: runExtractorInPage,
      });
      snapshot = results?.[0]?.result;
      if (snapshot) break;
    } catch (err) {
      console.warn(`[agenterm-webext] executeScript failed for ${provider.id}:`, err);
    }
  }

  // Open background tab if no snapshot yet
  if (!snapshot) {
    try {
      const tab = await chrome.tabs.create({ url: primaryUrl, active: false });
      await waitForTabComplete(tab.id, 45000);
      const results = await chrome.scripting.executeScript({
        target: { tabId: tab.id },
        func: runExtractorInPage,
      });
      snapshot = results?.[0]?.result;
      await chrome.tabs.remove(tab.id);
    } catch (err) {
      console.warn(`[agenterm-webext] background tab poll failed for ${provider.id}:`, err);
    }
  }

  if (snapshot) {
    await handleContentSnapshot(snapshot);
  }
}

function runExtractorInPage() {
  const href = window.location.href;
  const patterns = [
    { id: "cursor", prefixes: ["https://cursor.com/dashboard", "https://cursor.com/agents"] },
    { id: "grok", prefixes: ["https://grok.com", "https://accounts.x.ai"] },
    {
      id: "chatgpt",
      prefixes: [
        "https://chatgpt.com",
        "https://chat.openai.com",
        "https://platform.openai.com",
      ],
    },
  ];
  let providerId = null;
  for (const p of patterns) {
    if (p.prefixes.some((pre) => href.startsWith(pre))) {
      providerId = p.id;
      break;
    }
  }
  if (!providerId) return null;

  const text = document.body?.innerText ?? "";
  const limitsReached = /usage\s+limits?\s+reached|limit\s+reached/i.test(text);

  let remainingPct = null;
  let plan = null;
  let used = null;
  let limit = null;
  let resetAt = null;
  let onDemandSpend = null;

  if (providerId === "cursor") {
    const ultra = text.match(/\b(Ultra|Pro|Business|Hobby|Free)\b/i);
    if (ultra) plan = ultra[1];

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
    const spend = text.match(/on[- ]?demand[^$]{0,40}\$\s*([\d,]+(?:\.\d+)?)/i);
    if (spend) onDemandSpend = Number(spend[1].replace(/,/g, ""));

    const reset = text.match(/resets?\s+(?:on\s+)?([A-Za-z]+\s+\d{1,2},?\s+\d{4})/i);
    if (reset) resetAt = reset[1];
  }

  if (providerId === "grok") {
    const p = text.match(/\b(SuperGrok|Super Grok|Grok Pro|Premium|Free)\b/i);
    if (p) plan = p[1];
    const rem = text.match(/(\d+(?:\.\d+)?)\s*%\s*remaining/i);
    if (rem) remainingPct = Number(rem[1]);
    const of = text.match(/(\d+)\s*of\s*(\d+)\s*(?:requests?|credits?)/i);
    if (of) {
      used = Number(of[1]);
      limit = Number(of[2]);
      remainingPct = Math.max(0, Math.round(((limit - used) / limit) * 100));
    }
  }

  if (providerId === "chatgpt") {
    const planMatch = text.match(/ChatGPT\s+(Plus|Pro|Team|Business|Enterprise|Free)/i);
    if (planMatch) {
      plan = planMatch[1];
    } else {
      const plain = text.match(/\b(Business|Team|Enterprise|Pro|Plus|Free)\b/i);
      if (plain) plan = plain[1];
    }
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
    const reset = text.match(/resets?\s+(?:on\s+)?([A-Za-z]+\s+\d{1,2},?\s+\d{4})/i);
    if (reset) resetAt = reset[1];
    const billing = text.match(/billing\s+period\s+ends?\s+([A-Za-z]+\s+\d{1,2},?\s+\d{4})/i);
    if (billing) resetAt = billing[1];
  }

  if (limitsReached) remainingPct = 0;

  return {
    providerId,
    used,
    limit,
    remainingPct,
    resetAt,
    plan,
    limitsReached,
    raw: { url: href, onDemandSpend, snippet: text.slice(0, 300) },
    capturedAt: Date.now(),
  };
}

async function handleContentSnapshot(snapshot) {
  if (!snapshot?.providerId) return;

  await saveSnapshot(snapshot.providerId, snapshot);
  await maybeAlert(snapshot);
}

async function maybeAlert(snapshot) {
  const config = await getConfig();
  const providerCfg = config.providers[snapshot.providerId];
  if (!providerCfg?.enabled) return;

  const provider = providers.find((p) => p.id === snapshot.providerId);
  if (!provider) return;

  const threshold = providerCfg.thresholdPct ?? 15;
  const shouldAlert =
    snapshot.limitsReached ||
    (snapshot.remainingPct != null && snapshot.remainingPct <= threshold);

  if (!shouldAlert) return;

  const alertState = await getAlertState();
  const last = alertState[snapshot.providerId];
  const now = Date.now();
  if (last && now - last < ALERT_COOLDOWN_MS) return;

  const reason = snapshot.limitsReached
    ? "Usage limit reached"
    : `Remaining usage at ${snapshot.remainingPct}% (threshold ${threshold}%)`;

  await sendUsageAlert(config, provider, snapshot, reason);
  alertState[snapshot.providerId] = now;
  await setAlertState(alertState);
}

function toChromePattern(urlPattern) {
  if (urlPattern.endsWith("*")) return urlPattern;
  return urlPattern + "*";
}

function waitForTabComplete(tabId, timeoutMs) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      chrome.tabs.onUpdated.removeListener(listener);
      reject(new Error("tab load timeout"));
    }, timeoutMs);

    function listener(id, info) {
      if (id === tabId && info.status === "complete") {
        clearTimeout(timer);
        chrome.tabs.onUpdated.removeListener(listener);
        setTimeout(resolve, 1500);
      }
    }
    chrome.tabs.onUpdated.addListener(listener);
  });
}
