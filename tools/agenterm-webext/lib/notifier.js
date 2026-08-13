/**
 * Pluggable alert delivery: chrome.notifications (always available),
 * optional webhook (user-supplied URL), mailto draft fallback.
 * Gmail REST from the extension is unreliable; host/MCP can send Gmail later.
 */

export async function sendUsageAlert(config, provider, snapshot, reason) {
  const lines = formatAlertBody(provider, snapshot, reason);
  const title = `AgenTerm WebExt: ${provider.name}`;

  if (config.notificationsEnabled) {
    await chrome.notifications.create(`agenterm-webext-${provider.id}-${Date.now()}`, {
      type: "basic",
      iconUrl: "icons/icon128.png",
      title,
      message: lines.join("\n"),
      priority: 2,
      requireInteraction: true,
    });
  }

  if (config.webhookEnabled && config.webhookUrl) {
    await sendWebhook(config.webhookUrl, {
      to: config.emailDestination,
      subject: `[AgenTerm WebExt] ${provider.name} — ${reason}`,
      body: lines.join("\n"),
      providerId: provider.id,
      snapshot,
    });
  }

  if (config.mailtoEnabled && config.emailDestination) {
    const subject = encodeURIComponent(`[AgenTerm WebExt] ${provider.name} — ${reason}`);
    const body = encodeURIComponent(lines.join("\n"));
    const mailto = `mailto:${config.emailDestination}?subject=${subject}&body=${body}`;
    // Open as draft in default mail client; user sends manually.
    await chrome.tabs.create({ url: mailto, active: false });
  }
}

function formatAlertBody(provider, snapshot, reason) {
  const parts = [
    `Provider: ${provider.name}`,
    `Reason: ${reason}`,
    `Plan: ${snapshot.plan ?? "unknown"}`,
    `Remaining: ${snapshot.remainingPct != null ? `${snapshot.remainingPct}%` : "unknown"}`,
    `Used: ${snapshot.used ?? "?"}`,
    `Limit: ${snapshot.limit ?? "?"}`,
    `Limits reached banner: ${snapshot.limitsReached ? "yes" : "no"}`,
    `Reset: ${snapshot.resetAt ?? "unknown"}`,
    `Captured: ${new Date(snapshot.capturedAt).toLocaleString()}`,
  ];
  return parts;
}

async function sendWebhook(url, payload) {
  try {
    const response = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      console.warn("[agenterm-webext] webhook failed:", response.status, await response.text());
    }
  } catch (err) {
    console.warn("[agenterm-webext] webhook error:", err);
  }
}
