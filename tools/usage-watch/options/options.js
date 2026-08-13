import { getConfig, setConfig } from "../lib/storage.js";
import { providers } from "../providers/index.js";

const PROVIDER_LABELS = {
  cursor: "Cursor (dashboard / usage / agents)",
  grok: "Grok / xAI",
  anthropic: "Anthropic — stub",
  openai: "OpenAI — stub",
  github_copilot: "GitHub Copilot — stub",
};

async function load() {
  const config = await getConfig();
  document.getElementById("emailDestination").value = config.emailDestination;
  document.getElementById("pollIntervalMinutes").value = config.pollIntervalMinutes;
  document.getElementById("webhookUrl").value = config.webhookUrl;
  document.getElementById("webhookEnabled").checked = config.webhookEnabled;
  document.getElementById("mailtoEnabled").checked = config.mailtoEnabled;
  document.getElementById("notificationsEnabled").checked = config.notificationsEnabled;

  const list = document.getElementById("providerList");
  list.innerHTML = "";

  for (const provider of providers) {
    const pc = config.providers[provider.id] ?? { enabled: false, thresholdPct: 15 };
    const row = document.createElement("div");
    row.className = "provider-row";
    row.innerHTML = `
      <div class="provider-name">
        ${provider.name}
        <br><small>${PROVIDER_LABELS[provider.id] ?? provider.id}</small>
      </div>
      <label class="checkbox">
        <input type="checkbox" data-provider="${provider.id}" data-field="enabled" ${pc.enabled ? "checked" : ""} />
        On
      </label>
      <label>
        Threshold %
        <input type="number" min="0" max="100" data-provider="${provider.id}" data-field="thresholdPct" value="${pc.thresholdPct}" style="width:72px;margin-top:4px" />
      </label>
    `;
    list.appendChild(row);
  }
}

async function save() {
  const providersPatch = {};
  for (const provider of providers) {
    const enabled = document.querySelector(
      `input[data-provider="${provider.id}"][data-field="enabled"]`
    );
    const threshold = document.querySelector(
      `input[data-provider="${provider.id}"][data-field="thresholdPct"]`
    );
    providersPatch[provider.id] = {
      enabled: enabled?.checked ?? false,
      thresholdPct: Number(threshold?.value ?? 15),
    };
  }

  const config = await setConfig({
    emailDestination: document.getElementById("emailDestination").value.trim(),
    pollIntervalMinutes: Number(document.getElementById("pollIntervalMinutes").value) || 30,
    webhookUrl: document.getElementById("webhookUrl").value.trim(),
    webhookEnabled: document.getElementById("webhookEnabled").checked,
    mailtoEnabled: document.getElementById("mailtoEnabled").checked,
    notificationsEnabled: document.getElementById("notificationsEnabled").checked,
    providers: providersPatch,
  });

  chrome.runtime.sendMessage({ type: "reschedule" });
  document.getElementById("status").textContent = "Saved.";
  return config;
}

document.getElementById("saveBtn").addEventListener("click", () => save());
document.getElementById("pollNowBtn").addEventListener("click", async () => {
  await save();
  document.getElementById("status").textContent = "Polling…";
  chrome.runtime.sendMessage({ type: "poll-now" }, () => {
    document.getElementById("status").textContent = "Poll triggered.";
  });
});

load();
