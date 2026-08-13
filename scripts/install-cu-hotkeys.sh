#!/bin/sh
# Install agenterm-cu as the local Spectacle replacement (macOS).
# launchd needs absolute paths; this script expands $HOME at install time.

set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
BIN_DIR="${HOME}/.local/bin"
DATA_DIR="${HOME}/.local/share/agenterm"
LAUNCH_DIR="${HOME}/Library/LaunchAgents"
LABEL=com.agenterm.cu.hotkeys
APP="${DATA_DIR}/AgentermCu.app"
APP_BIN="${APP}/Contents/MacOS/agenterm-cu"
BIN="${BIN_DIR}/agenterm-cu"
PLIST="${LAUNCH_DIR}/${LABEL}.plist"

echo "building release cu..."
cargo build -p agenterm-cu --bin cu --release --manifest-path "${ROOT}/Cargo.toml"

mkdir -p "${BIN_DIR}" "${DATA_DIR}" "${LAUNCH_DIR}" "${APP}/Contents/MacOS"
cp "${ROOT}/target/release/cu" "${APP_BIN}"
chmod 755 "${APP_BIN}"
ln -sfn "${APP_BIN}" "${BIN}"

cat > "${APP}/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>com.agenterm.cu</string>
  <key>CFBundleName</key>
  <string>AgentermCu</string>
  <key>CFBundleExecutable</key>
  <string>agenterm-cu</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
EOF
printf 'APPL????' > "${APP}/Contents/PkgInfo"
codesign --force --deep --sign - "${APP}" >/dev/null

cat > "${PLIST}" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>${LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>${APP_BIN}</string>
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

echo "installed ${APP}"
echo "cli ${BIN}"
echo "launchd ${LABEL} loaded"
echo "enable Accessibility for: AgentermCu (${APP})"
open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
echo "Spectacle defaults: ⌥⌘←/→/↑/↓  ⌥⌘C/F/Z  ⌃⌘←/→  …"
