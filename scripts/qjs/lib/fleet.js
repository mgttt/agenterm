// fleet.js — Fleet broker wrapper for qjs scripts.
// Wraps __host.fleet_call(operation_id, params_json) -> result_json.
// Line-for-line port of scripts/lua/lib/fleet.lua: same operation_id
// strings, same params shape, so a script that calls fleet.tabs.set_note(...)
// produces the identical Fleet operation regardless of which engine ran it
// — see crates/agenterm-qjs/src/host.rs module doc and PRD "Script engine
// family" / "capability alignment" for why this is a deliberate port, not
// a reinvention. Uses QuickJS's native JSON (no std.json module needed,
// unlike lua's stdlib.lua wrapper).

const fleet = {};

function call(opId, params) {
  const resultJson = __host.fleet_call(opId, params === undefined ? "{}" : params);
  try {
    return JSON.parse(resultJson);
  } catch (_err) {
    return resultJson;
  }
}

// -- fleet.tabs ------------------------------------------------------------

fleet.tabs = {};

/** List all tabs. */
fleet.tabs.list = function () {
  return call("tabs.list");
};

/** Get the active tab. */
fleet.tabs.active = function () {
  return call("tabs.active");
};

/** Set a note on a tab. */
fleet.tabs.set_note = function (tabId, note) {
  return call("tabs.set-note", JSON.stringify({ tab_id: tabId, note: note }));
};

// -- fleet.terminal ----------------------------------------------------------

fleet.terminal = {};

/** Paste text into terminal. */
fleet.terminal.paste = function (text) {
  return call("terminal.paste", JSON.stringify({ text: text }));
};

// -- fleet.ui ----------------------------------------------------------------

fleet.ui = {};

/** Bootstrap the UI. */
fleet.ui.bootstrap = function () {
  return call("ui.bootstrap");
};

/** Get UI snapshot. */
fleet.ui.snapshot = function () {
  return call("ui.snapshot");
};

/** Get UI deltas since last snapshot. */
fleet.ui.deltas = function () {
  return call("ui.deltas");
};

fleet.ui.composer = {};

/** Send a composer message. */
fleet.ui.composer.send = function (text) {
  return call("ui.composer.send", JSON.stringify({ text: text }));
};

/** Hello / ping. */
fleet.ui.hello = function () {
  return call("ui.hello");
};

fleet.ui.input = {};

/** Send pointer event. */
fleet.ui.input.pointer = function (x, y, action) {
  return call("ui.input.pointer", JSON.stringify({ x: x, y: y, action: action }));
};

/** Send wheel event. */
fleet.ui.input.wheel = function (delta) {
  return call("ui.input.wheel", JSON.stringify({ delta: delta }));
};

fleet.ui.tab = {};

/** Open a new child tab. */
fleet.ui.tab.new_child = function () {
  return call("ui.tab.new-child");
};

/** Select a tab by id. */
fleet.ui.tab.select = function (id) {
  return call("ui.tab.select", JSON.stringify({ id: id }));
};

fleet.ui.tabs = {};

/** Hide tabs panel. */
fleet.ui.tabs.hide = function () {
  return call("ui.tabs.hide");
};

/** Show tabs panel. */
fleet.ui.tabs.show = function () {
  return call("ui.tabs.show");
};

/** Toggle tabs panel visibility. */
fleet.ui.tabs.toggle = function () {
  return call("ui.tabs.toggle");
};

/** Set tabs panel width. */
fleet.ui.tabs.set_width = function (width) {
  return call("ui.tabs.set-width", JSON.stringify({ width: width }));
};

fleet.ui.tree = {};

/** Toggle tree panel. */
fleet.ui.tree.toggle = function () {
  return call("ui.tree.toggle");
};

fleet.ui.window = {};

/** Activate the window. */
fleet.ui.window.activate = function () {
  return call("ui.window.activate");
};

// -- fleet.workspace ----------------------------------------------------------

fleet.workspace = {};

/** Get workspace info. */
fleet.workspace.info = function () {
  return call("workspace.info");
};

/** Shutdown the workspace. */
fleet.workspace.shutdown = function () {
  return call("workspace.shutdown");
};

// -- fleet.control_center -------------------------------------------------

fleet.control_center = {};

/** Open the control center. */
fleet.control_center.open = function () {
  return call("control-center.open");
};

/** Close the control center. */
fleet.control_center.close = function () {
  return call("control-center.close");
};

/** Get control center snapshot. */
fleet.control_center.snapshot = function () {
  return call("control-center.snapshot");
};

/** Get control center status. */
fleet.control_center.status = function () {
  return call("control-center.status");
};

// -- fleet.events -------------------------------------------------------------

fleet.events = {};

/** Read events buffer. */
fleet.events.read = function () {
  return call("events.read");
};

/** Wait for events (blocking). */
fleet.events.wait = function (timeoutMs) {
  return call("events.wait", JSON.stringify({ timeout_ms: timeoutMs }));
};

// -- fleet.protocol -------------------------------------------------------------

fleet.protocol = {};

/** Get protocol info. */
fleet.protocol.info = function () {
  return call("protocol.info");
};

// -- fleet.server -------------------------------------------------------------

fleet.server = {};

/** Kill the server. */
fleet.server.kill = function () {
  return call("server.kill");
};
