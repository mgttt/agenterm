/**
 * Stub: OpenAI / ChatGPT / Codex usage.
 * Add extractors when platform.openai.com or chatgpt.com usage UI is mapped.
 */
export const openaiProvider = {
  id: "openai",
  name: "OpenAI (ChatGPT / Codex)",
  matchUrls: [
    "https://platform.openai.com/*",
    "https://chatgpt.com/*",
    "https://chat.openai.com/*",
  ],

  extractFromDom(_document) {
    return null;
  },

  extractFromJson(_json) {
    return null;
  },
};
