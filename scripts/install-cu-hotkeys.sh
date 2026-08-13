#!/bin/sh
# Install AgentermCu as the local Spectacle replacement (macOS).
# launchd needs absolute paths; this script expands $HOME at install time.
#
# Accessibility is keyed to the *code signature* of this process, not just the
# name in System Settings. Ad-hoc re-sign changes the cdhash; Settings can still
# show ON while AXIsProcessTrusted is false. After each install we reset the
# com.agenterm.cu TCC entry so the UI never lies about a stale signature.

set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
BIN_DIR="${HOME}/.local/bin"
DATA_DIR="${HOME}/.local/share/agenterm"
APP_DIR="${HOME}/Applications"
LAUNCH_DIR="${HOME}/Library/LaunchAgents"
LABEL=com.agenterm.cu.hotkeys
APP="${APP_DIR}/AgentermCu.app"
# Keep a copy under data dir for older docs/paths; primary is ~/Applications.
LEGACY_APP="${DATA_DIR}/AgentermCu.app"
APP_BIN="${APP}/Contents/MacOS/agenterm-cu"
BIN="${BIN_DIR}/agenterm-cu"
PLIST="${LAUNCH_DIR}/${LABEL}.plist"

echo "building release cu..."
cargo build -p agenterm-cu --bin cu --release --manifest-path "${ROOT}/Cargo.toml"

mkdir -p "${BIN_DIR}" "${DATA_DIR}" "${LAUNCH_DIR}" "${APP}/Contents/MacOS"
cp "${ROOT}/target/release/cu" "${APP_BIN}"
chmod 755 "${APP_BIN}"
ln -sfn "${APP_BIN}" "${BIN}"

# Mirror for any old absolute paths.
mkdir -p "${LEGACY_APP}/Contents/MacOS"
cp "${APP_BIN}" "${LEGACY_APP}/Contents/MacOS/agenterm-cu"
chmod 755 "${LEGACY_APP}/Contents/MacOS/agenterm-cu"

write_plist() {
  local dest=$1
  cat > "${dest}/Contents/Info.plist" <<'EOF'
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
  <key>NSAccessibilityUsageDescription</key>
  <string>AgentermCu 需要辅助功能权限才能按快捷键移动其他窗口。请打开「AgentermCu」开关，不要选旧的 agenterm-cu。</string>
</dict>
</plist>
EOF
  printf 'APPL????' > "${dest}/Contents/PkgInfo"
}

write_plist "${APP}"
write_plist "${LEGACY_APP}"

# Sign both copies so their cdhash matches.
codesign --force --deep --sign - "${APP}" >/dev/null
codesign --force --deep --sign - "${LEGACY_APP}" >/dev/null

# Register with Launch Services so the Accessibility list shows AgentermCu.
if [ -x /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister ]; then
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "${APP}" >/dev/null 2>&1 || true
fi

# Drop stale ON state for this bundle id (wrong cdhash after re-sign).
tccutil reset Accessibility com.agenterm.cu >/dev/null 2>&1 || true

REQ=$(codesign -d -r- "${APP}" 2>&1 | sed -n 's/^# designated => //p' || true)
echo "signed ${APP}"
echo "designated: ${REQ}"

cat > "${PLIST}" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>${LABEL}</string>
  <key>AssociatedBundleIdentifiers</key>
  <array>
    <string>com.agenterm.cu</string>
  </array>
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
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
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

sleep 1
if [ -f "${DATA_DIR}/ax-status" ]; then
  echo "ax-status:"
  cat "${DATA_DIR}/ax-status"
fi

echo "installed ${APP}"
echo "cli ${BIN}"
echo "launchd ${LABEL} loaded"
echo "menu bar: AgentermCu — first item is Accessibility"
echo "IMPORTANT: enable AgentermCu in Accessibility once after this install"
echo "Spectacle defaults: ⌥⌘←/→/↑/↓  ⌥⌘C/F/Z  ⌃⌘←/→  …"
