# Usage Watch

Chrome/Chromium Manifest V3 extension that monitors AI provider usage quotas and alerts when remaining usage is low.

**Owner:** WJ C (`wanjochan@gmail.com`)

## Install (Load unpacked)

1. Open `chrome://extensions`
2. Enable **Developer mode**
3. Click **Load unpacked**
4. Select this folder: `tools/usage-watch` (from the repository root)

No build step required for the extension itself. Icons and scripts load directly.

## Stay signed in

Keep browser sessions active on the dashboards you enable:

| Provider | Pages |
|----------|--------|
| **Cursor** | `https://cursor.com/dashboard`, `/dashboard/usage`, `/dashboard/spending`, `https://cursor.com/agents` |
| **Grok / xAI** | `https://grok.com`, `https://accounts.x.ai` |
| Stubs (off by default) | Anthropic console, OpenAI platform, GitHub Copilot settings |

The extension reads the DOM (and embedded JSON when present). It does not store passwords or tokens.

## Options

Open from the extension icon → **Options**, or `chrome://extensions` → Usage Watch → **Extension options**.

- **Email destination** — default `wanjochan@gmail.com`
- **Threshold %** — alert when *remaining* usage ≤ this (default 15%), or when a “limit reached” banner is detected
- **Poll interval** — default 30 minutes (background alarm + optional tab poll)
- **Per-provider** enable/disable

### Alert channels

1. **Chrome notifications** (default, always available)
2. **mailto draft** — opens your mail client with a pre-filled draft (you send manually)
3. **Webhook URL** — paste a Mailgun / Resend / custom relay endpoint (no SMTP credentials in the extension)
4. **Host / MCP** — Grok Bot or other agents can send Gmail via MCP separately; this extension does not embed Gmail REST

## How it works

- **Content scripts** run on matched provider pages and push snapshots when the DOM changes.
- **Background service worker** runs a `chrome.alarm` every N minutes, reuses open tabs or opens background tabs, executes extraction, stores results in `chrome.storage.local`, and fires alerts when thresholds are crossed (with a 6-hour cooldown per provider).

## Add a new provider

1. Create `providers/your-provider.js`:

```js
export const yourProvider = {
  id: "your_id",
  name: "Display Name",
  matchUrls: ["https://example.com/usage*"],
  extractFromDom(document) {
    // return snapshot or null
    return {
      providerId: "your_id",
      used, limit, remainingPct, resetAt, plan,
      limitsReached: false,
      raw: {},
      capturedAt: Date.now(),
    };
  },
  extractFromJson(json) { return null; },
};
```

2. Register in `providers/index.js` (import + add to `providers` array).
3. Add `host_permissions` in `manifest.json` for the new origins.
4. Optionally add a `content_scripts` match block (or rely on background `executeScript`).
5. Add default config in `lib/types.js` under `providers`.
6. Add a fixture HTML under `fixtures/` and a test case in `tests/extractors.test.js`.

## Tests

```bash
cd tools/usage-watch
npm install
npm test
```

Tests parse redacted fixture HTML with jsdom — no live credentials or tokens.

## Privacy

- No analytics or third-party tracking
- No secrets shipped in the repo
- Snapshots stored locally in `chrome.storage.local` only

## Tuning extractors

Cursor and Grok UIs change often. After a live pass, adjust regex/selectors in:

- `providers/cursor.js`
- `providers/grok.js`
- `content/content.js` (content-script duplicate for live pages)
- `background.js` (`runExtractorInPage` for alarm polls)

Comments in those files mark best-effort selectors that may need updates.
