/**
 * Stub: Anthropic / Claude console usage.
 * Add extractFromDom when console.anthropic.com usage page structure is known.
 */
export const anthropicProvider = {
  id: "anthropic",
  name: "Anthropic (Claude)",
  matchUrls: [
    "https://console.anthropic.com/*",
  ],

  extractFromDom(_document) {
  return null;
  },

  extractFromJson(_json) {
    return null;
  },
};
