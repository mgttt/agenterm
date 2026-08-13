/**
 * @typedef {Object} UsageSnapshot
 * @property {string} providerId
 * @property {number|null} used
 * @property {number|null} limit
 * @property {number|null} remainingPct - 0–100 remaining (not used %)
 * @property {string|null} resetAt - ISO date or human-readable reset hint
 * @property {string|null} plan
 * @property {boolean} limitsReached
 * @property {Record<string, unknown>} raw
 * @property {number} capturedAt - epoch ms
 */

/**
 * @typedef {Object} ProviderConfig
 * @property {boolean} enabled
 * @property {number} thresholdPct - alert when remainingPct <= this (default 15)
 */

/**
 * @typedef {Object} AppConfig
 * @property {string} emailDestination
 * @property {number} pollIntervalMinutes
 * @property {string} webhookUrl
 * @property {boolean} webhookEnabled
 * @property {boolean} mailtoEnabled
 * @property {boolean} notificationsEnabled
 * @property {Record<string, ProviderConfig>} providers
 */

export const DEFAULT_CONFIG = {
  emailDestination: "wanjochan@gmail.com",
  pollIntervalMinutes: 30,
  webhookUrl: "",
  webhookEnabled: false,
  mailtoEnabled: true,
  notificationsEnabled: true,
  providers: {
    cursor: { enabled: true, thresholdPct: 15 },
    grok: { enabled: true, thresholdPct: 15 },
    chatgpt: { enabled: true, thresholdPct: 15 },
    anthropic: { enabled: false, thresholdPct: 15 },
    github_copilot: { enabled: false, thresholdPct: 15 },
  },
};
