const NAMES = {
  cursor: "Cursor",
  grok: "Grok",
  chatgpt: "ChatGPT",
  anthropic: "Anthropic",
  github_copilot: "Copilot",
};

chrome.storage.local.get("snapshots", (result) => {
  const snapshots = result.snapshots ?? {};
  const el = document.getElementById("snapshots");
  if (Object.keys(snapshots).length === 0) {
    el.textContent = "No snapshots yet. Visit a provider page or run Poll now.";
    return;
  }
  el.innerHTML = Object.entries(snapshots)
    .map(([id, s]) => {
      const rem = s.remainingPct != null ? `${s.remainingPct}% left` : "—";
      const plan = s.plan ?? "?";
      const when = new Date(s.capturedAt).toLocaleString();
      return `<div style="margin-bottom:8px;font-size:13px"><strong>${NAMES[id] ?? id}</strong> ${plan}<br>${rem}${s.limitsReached ? " ⚠ limit" : ""}<br><small>${when}</small></div>`;
    })
    .join("");
});

document.getElementById("openOptions").addEventListener("click", (e) => {
  e.preventDefault();
  chrome.runtime.openOptionsPage();
});
