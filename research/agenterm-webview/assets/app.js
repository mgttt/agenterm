"use strict";

// Deliberately DOM-only: no host bridge, external I/O, navigation, or storage APIs.
const state = document.getElementById("projection-state");
if (state) {
  state.dataset.assetVersion = "cockpit-placeholder/1";
}
