"use strict";

// DOM-only tab chrome for the size/spike shell. No host bridge, network, or storage.
(function () {
  const tabs = Array.from(document.querySelectorAll(".tab[data-tab]"));
  const panels = {
    agent: document.getElementById("panel-agent"),
    hub: document.getElementById("panel-hub"),
    control: document.getElementById("panel-control"),
  };
  const badge = document.getElementById("host-badge");
  const asset = document.getElementById("asset-version");

  if (badge) {
    badge.dataset.assetVersion = "cc-shell-placeholder/2";
  }
  if (asset) {
    asset.textContent = "cc-shell-placeholder/2";
  }

  function selectTab(name) {
    const panel = panels[name];
    if (!panel) {
      return;
    }
    tabs.forEach((tab) => {
      const active = tab.dataset.tab === name;
      tab.classList.toggle("is-active", active);
      tab.setAttribute("aria-selected", active ? "true" : "false");
    });
    Object.entries(panels).forEach(([key, node]) => {
      if (!node) {
        return;
      }
      const active = key === name;
      node.classList.toggle("is-active", active);
      node.hidden = !active;
    });
  }

  tabs.forEach((tab) => {
    tab.addEventListener("click", () => selectTab(tab.dataset.tab));
  });

  selectTab("agent");
})();
