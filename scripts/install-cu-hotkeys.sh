#!/bin/sh
# Install agenterm-cu as the local Spectacle replacement (macOS).
# launchd needs absolute paths; this script expands $HOME at install time.

set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
BIN_DIR="${HOME}/.local/bin"
DATA_DIR="${HOME}/.local/share/agenterm"
LAUNCH_DIR="${HOME}/Library/LaunchAgents"
LABEL=com.agenterm.cu.hotkeys
BIN="${BIN_DIR}/agenterm-cu"
PLIST="${LAUNCH_DIR}/${LABEL}.plist"

echo "building release cu..."
cargo build -p agenterm-cu --bin cu --release --manifest-path "${ROOT}/Cargo.toml"

mkdir -p "${BIN_DIR}" "${DATA_DIR}" "${LAUNCH_DIR}"
cp "${ROOT}/target/release/cu" "${BIN}"
chmod 755 "${BIN}"

cat > "${PLIST}" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>${LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>${BIN}</string>
    <string>hotkeys</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>AGENTERM_CU_GRANT</key>
    <string>observe,actuate</string>
    <key>AGENTERM_CU_AUDIT_PATH</key>
    <string>${DATA_DIR}/cu-audit.jsonl</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>LimitLoadToSessionType</key>
  <string>Aqua</string>
  <key>ProcessType</key>
  <string>Interactive</string>
  <key>StandardOutPath</key>
  <string>${DATA_DIR}/cu-hotkeys.log</string>
  <key>StandardErrorPath</key>
  <string>${DATA_DIR}/cu-hotkeys.log</string>
</dict>
</plist>
EOF

launchctl bootout "gui/$(id -u)/${LABEL}" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "${PLIST}"
launchctl enable "gui/$(id -u)/${LABEL}"
launchctl kickstart -k "gui/$(id -u)/${LABEL}"

echo "installed ${BIN}"
echo "launchd ${LABEL} loaded"
echo "enable Accessibility for: ${BIN}"
open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
echo "Spectacle defaults: ⌥⌘←/→/↑/↓  ⌥⌘C/F/Z  ⌃⌘←/→  …"
