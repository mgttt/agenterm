/**
 * Stub: GitHub Copilot usage.
 * Add extractors when github.com/settings/copilot usage DOM is mapped.
 */
export const githubCopilotProvider = {
  id: "github_copilot",
  name: "GitHub Copilot",
  matchUrls: [
    "https://github.com/settings/copilot*",
    "https://github.com/settings/billing*",
  ],

  extractFromDom(_document) {
    return null;
  },

  extractFromJson(_json) {
    return null;
  },
};
