import { cursorProvider } from "./cursor.js";
import { grokProvider } from "./grok.js";
import { chatgptProvider } from "./chatgpt.js";
import { anthropicProvider } from "./stub-anthropic.js";
import { githubCopilotProvider } from "./stub-github.js";

/** All registered providers (usage modules and future capabilities). Add a file + import here. */
export const providers = [
  cursorProvider,
  grokProvider,
  chatgptProvider,
  anthropicProvider,
  githubCopilotProvider,
];

export function getProvider(id) {
  return providers.find((p) => p.id === id);
}

export function matchProviderForUrl(url) {
  try {
    const u = new URL(url);
    for (const provider of providers) {
      for (const pattern of provider.matchUrls) {
        if (urlMatchesPattern(u, pattern)) return provider;
      }
    }
  } catch {
    return null;
  }
  return null;
}

function urlMatchesPattern(url, pattern) {
  if (pattern.endsWith("*")) {
    const prefix = pattern.slice(0, -1);
    const href = url.href;
    return href.startsWith(prefix) || href.startsWith(prefix.replace(/\/$/, ""));
  }
  return url.href === pattern || url.origin + url.pathname === pattern;
}

export function extractFromDocumentSync(provider, document) {
  let snapshot = provider.extractFromDom(document);
  if (!snapshot) {
    const scripts = document.querySelectorAll(
      "script[type='application/json'], script#__NEXT_DATA__"
    );
    for (const script of scripts) {
      try {
        const json = JSON.parse(script.textContent);
        const fromJson = provider.extractFromJson?.(json);
        if (fromJson) {
          snapshot = fromJson;
          break;
        }
      } catch {
        // skip
      }
    }
  }
  return snapshot;
}
