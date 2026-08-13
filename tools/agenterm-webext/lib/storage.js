import { DEFAULT_CONFIG } from "./types.js";

const CONFIG_KEY = "config";
const SNAPSHOTS_KEY = "snapshots";
const ALERT_STATE_KEY = "alertState";

export async function getConfig() {
  const result = await chrome.storage.local.get(CONFIG_KEY);
  const stored = result[CONFIG_KEY] ?? {};
  return mergeConfig(DEFAULT_CONFIG, stored);
}

export async function setConfig(partial) {
  const current = await getConfig();
  const next = mergeConfig(current, partial);
  await chrome.storage.local.set({ [CONFIG_KEY]: next });
  return next;
}

export async function getSnapshots() {
  const result = await chrome.storage.local.get(SNAPSHOTS_KEY);
  return result[SNAPSHOTS_KEY] ?? {};
}

export async function saveSnapshot(providerId, snapshot) {
  const snapshots = await getSnapshots();
  snapshots[providerId] = snapshot;
  await chrome.storage.local.set({ [SNAPSHOTS_KEY]: snapshots });
  return snapshots;
}

export async function getAlertState() {
  const result = await chrome.storage.local.get(ALERT_STATE_KEY);
  return result[ALERT_STATE_KEY] ?? {};
}

export async function setAlertState(state) {
  await chrome.storage.local.set({ [ALERT_STATE_KEY]: state });
}

function mergeConfig(base, override) {
  const merged = { ...base, ...override };
  merged.providers = { ...base.providers, ...(override.providers ?? {}) };
  for (const id of Object.keys(base.providers)) {
    merged.providers[id] = {
      ...base.providers[id],
      ...(override.providers?.[id] ?? {}),
    };
  }
  return merged;
}
