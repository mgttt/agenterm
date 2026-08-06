#!/usr/bin/env bash
#
# Install AgenTerm from a GitHub Release or a local macOS build.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/mgttt/agenterm/main/install.sh | bash
#   ./install.sh --local-build target/debug
#
# Optional environment:
#   AGENTERM_VERSION=v0.1.10
#   AGENTERM_INSTALL_DIR=$HOME/.local/share/agenterm
#   AGENTERM_BIN_DIR=$HOME/.local/bin
#   AGENTERM_NO_LAUNCH=1
#   AGENTERM_ALLOW_UNSIGNED_PREVIEW=1

set -euo pipefail

REPOSITORY="mgttt/agenterm"
GITHUB_URL="https://github.com"
INSTALL_ROOT="${AGENTERM_INSTALL_DIR:-${HOME:?HOME is not set}/.local/share/agenterm}"
BIN_DIR="${AGENTERM_BIN_DIR:-${HOME}/.local/bin}"
APPLICATIONS_DIR="${AGENTERM_APPLICATIONS_DIR:-${HOME}/Applications}"
NO_LAUNCH="${AGENTERM_NO_LAUNCH:-0}"
ALLOW_UNSIGNED_PREVIEW="${AGENTERM_ALLOW_UNSIGNED_PREVIEW:-0}"
DOWNLOAD_BASE="${AGENTERM_DOWNLOAD_BASE:-}"
TMP_DIR=""
LOCAL_BUILD_DIR=""

say() {
  printf '==> %s\n' "$*"
}

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  if [[ -n "$TMP_DIR" && -d "$TMP_DIR" ]]; then
    rm -rf "$TMP_DIR"
  fi
}

trap cleanup EXIT HUP INT TERM

usage() {
  cat <<'EOF'
Install AgenTerm from GitHub Releases or a local macOS build.

Usage:
  ./install.sh
  ./install.sh --local-build BINARY_DIR

Environment variables:
  AGENTERM_VERSION                  Install a specific tag, for example v0.1.10
  AGENTERM_INSTALL_DIR              Payload root (default: ~/.local/share/agenterm)
  AGENTERM_BIN_DIR                  Command symlink directory (default: ~/.local/bin)
  AGENTERM_APPLICATIONS_DIR         macOS app directory (default: ~/Applications)
  AGENTERM_NO_LAUNCH=1              Install without starting the GUI
  AGENTERM_ALLOW_UNSIGNED_PREVIEW=1 Permit a labeled macOS unsigned preview
  AGENTERM_RELEASES_KEEP=N          Keep current + (N-1) prior release dirs (default 2)
EOF
}

case "${1:-}" in
  --help | -h)
    usage
    exit 0
    ;;
  --local-build)
    [[ $# -eq 2 ]] || fail "--local-build requires exactly one binary directory"
    LOCAL_BUILD_DIR="$2"
    ;;
  "")
    ;;
  *)
    fail "unexpected argument: $1"
    ;;
esac

command -v mktemp >/dev/null 2>&1 || fail "mktemp is required"
if [[ -z "$LOCAL_BUILD_DIR" ]]; then
  command -v curl >/dev/null 2>&1 || fail "curl is required"
fi

case "$(uname -s)" in
  Darwin)
    OS="macos"
    ARCHIVE_EXTENSION="zip"
    ;;
  Linux)
    OS="linux"
    ARCHIVE_EXTENSION="tar.gz"
    ;;
  *)
    fail "unsupported operating system: $(uname -s)"
    ;;
esac

case "$(uname -m)" in
  arm64 | aarch64)
    ARCH="aarch64"
    ;;
  x86_64 | amd64)
    ARCH="x86_64"
    ;;
  *)
    fail "unsupported CPU architecture: $(uname -m)"
    ;;
esac

download() {
  local url="$1"
  local destination="$2"
  local -a options=(--fail --silent --show-error --location --retry 3)
  if [[ "$url" == https://* ]]; then
    options+=(--proto '=https' --tlsv1.2)
  fi
  curl "${options[@]}" --output "$destination" "$url"
}

replace_symlink() {
  local target="$1"
  local link_path="$2"
  local next_link="${link_path}.next.$$"
  if [[ -e "$link_path" && ! -L "$link_path" ]]; then
    fail "refusing to replace a non-symlink path: $link_path"
  fi
  ln -s "$target" "$next_link"
  if [[ "$OS" == "macos" ]]; then
    mv -fh "$next_link" "$link_path"
  else
    mv -Tf "$next_link" "$link_path"
  fi
}

resolve_version() {
  local effective_url
  if [[ -n "${AGENTERM_VERSION:-}" ]]; then
    printf '%s\n' "$AGENTERM_VERSION"
    return
  fi
  effective_url="$(
    curl --fail --silent --show-error --location \
      --proto '=https' --tlsv1.2 \
      --output /dev/null --write-out '%{url_effective}' \
      "$GITHUB_URL/$REPOSITORY/releases/latest"
  )"
  [[ "$effective_url" == */tag/* ]] ||
    fail "GitHub did not resolve a latest release"
  printf '%s\n' "${effective_url##*/}"
}

if [[ -n "$LOCAL_BUILD_DIR" ]]; then
  [[ "$OS" == "macos" ]] || fail "--local-build currently supports macOS only"
  [[ -d "$LOCAL_BUILD_DIR" ]] || fail "local build directory does not exist: $LOCAL_BUILD_DIR"
  LOCAL_BUILD_DIR="$(cd "$LOCAL_BUILD_DIR" && pwd -P)"
  REQUIRED_EXECUTABLES=(agenterm agenterm-cli agenterm-rhai)
  for executable in "${REQUIRED_EXECUTABLES[@]}"; do
    SOURCE_PATH="$LOCAL_BUILD_DIR/$executable"
    [[ -f "$SOURCE_PATH" && ! -L "$SOURCE_PATH" && -x "$SOURCE_PATH" ]] ||
      fail "local build is missing executable: $SOURCE_PATH"
  done
  LOCAL_VERSION_OUTPUT="$($LOCAL_BUILD_DIR/agenterm-cli --version)"
  [[ "$LOCAL_VERSION_OUTPUT" =~ ^agenterm-cli[[:space:]]+([0-9A-Za-z.+_-]+)$ ]] ||
    fail "local agenterm-cli returned an invalid version: $LOCAL_VERSION_OUTPUT"
  RELEASE_VERSION="${BASH_REMATCH[1]}"
  VERSION="v$RELEASE_VERSION-local"
  TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/agenterm-install.XXXXXXXX")"
  STAGING_DIR="$TMP_DIR/payload"
  mkdir -p "$STAGING_DIR"
  for executable in "${REQUIRED_EXECUTABLES[@]}"; do
    cp "$LOCAL_BUILD_DIR/$executable" "$STAGING_DIR/$executable"
  done

  RELEASES_DIR="$INSTALL_ROOT/releases"
  RELEASE_DIR="$RELEASES_DIR/$RELEASE_VERSION-local-$OS-$ARCH"
  mkdir -p "$RELEASES_DIR" "$BIN_DIR"
  if [[ -e "$RELEASE_DIR" || -L "$RELEASE_DIR" ]]; then
    rm -rf "$RELEASE_DIR"
  fi
  mv "$STAGING_DIR" "$RELEASE_DIR"
  CURRENT_LINK="$INSTALL_ROOT/current"
  replace_symlink "$RELEASE_DIR" "$CURRENT_LINK"
  for executable in "${REQUIRED_EXECUTABLES[@]}"; do
    replace_symlink "$CURRENT_LINK/$executable" "$BIN_DIR/$executable"
  done

  APP_DIR="$APPLICATIONS_DIR/AgenTerm.app"
  APP_CONTENTS="$APP_DIR/Contents"
  mkdir -p "$APP_CONTENTS/MacOS" "$APP_CONTENTS/Resources"
  cp "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)/assets/agenterm.icns" \
    "$APP_CONTENTS/Resources/AgenTerm.icns"
  rm -f "$APP_CONTENTS/MacOS/AgenTerm"
  ln -s "$CURRENT_LINK/agenterm" "$APP_CONTENTS/MacOS/AgenTerm"
  cat >"$APP_CONTENTS/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key>
  <string>AgenTerm</string>
  <key>CFBundleExecutable</key>
  <string>AgenTerm</string>
  <key>CFBundleIdentifier</key>
  <string>tech.mega.agenterm</string>
  <key>CFBundleIconFile</key>
  <string>AgenTerm.icns</string>
  <key>CFBundleName</key>
  <string>AgenTerm</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$RELEASE_VERSION</string>
</dict>
</plist>
EOF
  INSTALLED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || date -u)"
  cat >"$RELEASE_DIR/installed.json" <<EOF
{
  "schema_version": 1,
  "version": "$RELEASE_VERSION",
  "tag": "$VERSION",
  "channel": "local-build",
  "variant": "$OS-$ARCH-local",
  "source_commit": "",
  "sha256": "",
  "installed_at": "$INSTALLED_AT",
  "os": "$OS",
  "arch": "$ARCH"
}
EOF
  say "Installed local AgenTerm $RELEASE_VERSION to $RELEASE_DIR"
  say "Install record: $CURRENT_LINK/installed.json"
  say "Commands are available in $BIN_DIR"
  say "Dock application is available at $APP_DIR"
  if [[ "$NO_LAUNCH" != "1" ]]; then
    say "Launching AgenTerm"
    open "$APP_DIR"
  fi
  exit 0
fi

VERSION="$(resolve_version)"
[[ "$VERSION" =~ ^v[0-9A-Za-z.+_-]+$ ]] ||
  fail "invalid release tag: $VERSION"
RELEASE_VERSION="${VERSION#v}"

if [[ -z "$DOWNLOAD_BASE" ]]; then
  DOWNLOAD_BASE="$GITHUB_URL/$REPOSITORY/releases/download/$VERSION"
fi

PACKAGE_STEM="agenterm-${RELEASE_VERSION}-${OS}-${ARCH}"
if [[ "$OS" == "macos" && "$ALLOW_UNSIGNED_PREVIEW" == "1" ]]; then
  PACKAGE_STEM="${PACKAGE_STEM}-unsigned-preview"
fi
ARCHIVE_NAME="${PACKAGE_STEM}.${ARCHIVE_EXTENSION}"
ARCHIVE_URL="${DOWNLOAD_BASE%/}/$ARCHIVE_NAME"
CHECKSUM_URL="${ARCHIVE_URL}.sha256"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/agenterm-install.XXXXXXXX")"
ARCHIVE_PATH="$TMP_DIR/$ARCHIVE_NAME"
CHECKSUM_PATH="$ARCHIVE_PATH.sha256"
STAGING_DIR="$TMP_DIR/payload"
mkdir -p "$STAGING_DIR"

say "Downloading AgenTerm $VERSION for $OS-$ARCH"
if ! download "$ARCHIVE_URL" "$ARCHIVE_PATH"; then
  if [[ "$OS" == "macos" && "$ALLOW_UNSIGNED_PREVIEW" != "1" ]]; then
    fail "signed macOS asset is unavailable; set AGENTERM_ALLOW_UNSIGNED_PREVIEW=1 only if you accept the developer-preview trust model"
  fi
  fail "release asset is unavailable: $ARCHIVE_URL"
fi
download "$CHECKSUM_URL" "$CHECKSUM_PATH" ||
  fail "release checksum is unavailable: $CHECKSUM_URL"

EXPECTED_SHA256="$(awk 'NR == 1 { print $1 }' "$CHECKSUM_PATH")"
[[ "$EXPECTED_SHA256" =~ ^[0-9a-fA-F]{64}$ ]] ||
  fail "release checksum has an invalid format"
if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SHA256="$(sha256sum "$ARCHIVE_PATH" | awk '{ print $1 }')"
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL_SHA256="$(shasum -a 256 "$ARCHIVE_PATH" | awk '{ print $1 }')"
else
  fail "sha256sum or shasum is required"
fi
NORMALIZED_ACTUAL_SHA256="$(printf '%s' "$ACTUAL_SHA256" | tr '[:upper:]' '[:lower:]')"
NORMALIZED_EXPECTED_SHA256="$(printf '%s' "$EXPECTED_SHA256" | tr '[:upper:]' '[:lower:]')"
[[ "$NORMALIZED_ACTUAL_SHA256" == "$NORMALIZED_EXPECTED_SHA256" ]] ||
  fail "SHA-256 verification failed for $ARCHIVE_NAME"
say "Verified SHA-256: $NORMALIZED_ACTUAL_SHA256"

if [[ "$OS" == "macos" ]]; then
  command -v ditto >/dev/null 2>&1 || fail "ditto is required on macOS"
  ditto -x -k "$ARCHIVE_PATH" "$STAGING_DIR"
else
  command -v tar >/dev/null 2>&1 || fail "tar is required on Linux"
  tar -xzf "$ARCHIVE_PATH" -C "$STAGING_DIR"
fi

REQUIRED_EXECUTABLES=(
  agenterm
  agenterm-cli
  agenterm-rhai
)
for executable in "${REQUIRED_EXECUTABLES[@]}"; do
  [[ -f "$STAGING_DIR/$executable" && ! -L "$STAGING_DIR/$executable" ]] ||
    fail "release payload is missing $executable"
  chmod +x "$STAGING_DIR/$executable"
done

if [[ "$OS" == "macos" && "$ALLOW_UNSIGNED_PREVIEW" != "1" ]]; then
  command -v codesign >/dev/null 2>&1 || fail "codesign is required on macOS"
  for executable in "${REQUIRED_EXECUTABLES[@]}"; do
    codesign --verify --strict "$STAGING_DIR/$executable" >/dev/null 2>&1 ||
      fail "Apple code-signature verification failed for $executable"
    SIGNATURE_INFO="$(codesign --display --verbose=4 "$STAGING_DIR/$executable" 2>&1)"
    [[ "$SIGNATURE_INFO" == *"Authority=Developer ID Application:"* ]] ||
      fail "$executable is not signed with an Apple Developer ID Application identity"
  done
fi

RELEASES_DIR="$INSTALL_ROOT/releases"
RELEASE_DIR="$RELEASES_DIR/$RELEASE_VERSION-$OS-$ARCH"
mkdir -p "$RELEASES_DIR" "$BIN_DIR"
if [[ -e "$RELEASE_DIR" || -L "$RELEASE_DIR" ]]; then
  rm -rf "$RELEASE_DIR"
fi
mv "$STAGING_DIR" "$RELEASE_DIR"

CURRENT_LINK="$INSTALL_ROOT/current"
replace_symlink "$RELEASE_DIR" "$CURRENT_LINK"

for executable in "${REQUIRED_EXECUTABLES[@]}"; do
  LINK_PATH="$BIN_DIR/$executable"
  replace_symlink "$CURRENT_LINK/$executable" "$LINK_PATH"
done

# G2: remove broken BIN symlinks that still point under this install root
# (e.g. renamed agenterm-script → agenterm-rhai left a dangling link).
if [[ -d "$BIN_DIR" ]]; then
  for link in "$BIN_DIR"/*; do
    [[ -L "$link" ]] || continue
    target="$(readlink "$link" 2>/dev/null || true)"
    [[ -n "$target" ]] || continue
    case "$target" in
      "$INSTALL_ROOT"/* | "$CURRENT_LINK"/* | */agenterm/*)
        if [[ ! -e "$link" ]]; then
          say "Removing broken install symlink: $link -> $target"
          rm -f "$link"
        fi
        ;;
    esac
  done
fi

# G6: keep current release dir + (N-1) newest others (default N=2).
# Override with AGENTERM_RELEASES_KEEP. Never delete the just-installed dir or
# anything still referenced by a BIN symlink.
prune_old_releases() {
  local keep="${AGENTERM_RELEASES_KEEP:-2}"
  [[ "$keep" =~ ^[1-9][0-9]*$ ]] || keep=2
  local max_old=$((keep - 1))
  [[ "$max_old" -lt 0 ]] && max_old=0

  local current_real
  current_real="$(cd "$RELEASE_DIR" && pwd -P 2>/dev/null || echo "$RELEASE_DIR")"

  local -a rows=()
  local dir mtime ref target referenced
  shopt -s nullglob
  for dir in "$RELEASES_DIR"/*; do
    [[ -d "$dir" && ! -L "$dir" ]] || continue
    local dir_real
    dir_real="$(cd "$dir" && pwd -P 2>/dev/null || echo "$dir")"
    [[ "$dir_real" == "$current_real" ]] && continue
    referenced=0
    if [[ -d "$BIN_DIR" ]]; then
      for ref in "$BIN_DIR"/*; do
        [[ -L "$ref" || -e "$ref" ]] || continue
        if command -v readlink >/dev/null 2>&1; then
          target="$(readlink -f "$ref" 2>/dev/null || true)"
        else
          target=""
        fi
        case "$target" in
          "$dir_real"/* | "$dir"/*) referenced=1; break ;;
        esac
      done
    fi
    [[ "$referenced" -eq 1 ]] && continue
    mtime="$(stat -c %Y "$dir" 2>/dev/null || stat -f %m "$dir" 2>/dev/null || echo 0)"
    rows+=("$mtime|$dir")
  done
  shopt -u nullglob

  if [[ ${#rows[@]} -eq 0 ]]; then
    return 0
  fi
  local sorted n=0
  sorted="$(printf '%s\n' "${rows[@]}" | sort -t'|' -k1,1nr)"
  while IFS= read -r row; do
    [[ -n "$row" ]] || continue
    dir="${row#*|}"
    n=$((n + 1))
    if [[ "$n" -le "$max_old" ]]; then
      continue
    fi
    say "Pruning old release directory (keep=$keep): $dir"
    rm -rf "$dir"
  done <<<"$sorted"
}
prune_old_releases

if [[ "$OS" == "macos" ]]; then
  APP_DIR="$APPLICATIONS_DIR/AgenTerm.app"
  APP_CONTENTS="$APP_DIR/Contents"
  mkdir -p "$APP_CONTENTS/MacOS" "$APP_CONTENTS/Resources"
  rm -f "$APP_CONTENTS/MacOS/AgenTerm"
  ln -s "$CURRENT_LINK/agenterm" "$APP_CONTENTS/MacOS/AgenTerm"
  cat >"$APP_CONTENTS/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key>
  <string>AgenTerm</string>
  <key>CFBundleExecutable</key>
  <string>AgenTerm</string>
  <key>CFBundleIdentifier</key>
  <string>tech.mega.agenterm</string>
  <key>CFBundleName</key>
  <string>AgenTerm</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$RELEASE_VERSION</string>
</dict>
</plist>
EOF
  GUI_PATH="$APP_DIR"
else
  GUI_PATH="$CURRENT_LINK/agenterm"
fi

# G3: machine-readable install record for version observability.
INSTALLED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || date -u)"
CHANNEL="release"
VARIANT="$OS-$ARCH"
if [[ "$OS" == "macos" && "$ALLOW_UNSIGNED_PREVIEW" == "1" ]]; then
  CHANNEL="macos-unsigned-preview"
  VARIANT="${VARIANT}-unsigned-preview"
fi
SOURCE_COMMIT=""
if [[ -f "$RELEASE_DIR/agenterm.provenance.json" ]]; then
  SOURCE_COMMIT="$(
    python3 -c "import json,sys; print(json.load(open(sys.argv[1])).get('source_commit',''))" \
      "$RELEASE_DIR/agenterm.provenance.json" 2>/dev/null || true
  )"
fi
# Prefer archive provenance next to downloaded artifact name if present.
if [[ -z "$SOURCE_COMMIT" && -f "$ARCHIVE_PATH.provenance.json" ]]; then
  SOURCE_COMMIT="$(
    python3 -c "import json,sys; print(json.load(open(sys.argv[1])).get('source_commit',''))" \
      "$ARCHIVE_PATH.provenance.json" 2>/dev/null || true
  )"
fi
cat >"$RELEASE_DIR/installed.json" <<EOF
{
  "schema_version": 1,
  "version": "$RELEASE_VERSION",
  "tag": "$VERSION",
  "channel": "$CHANNEL",
  "variant": "$VARIANT",
  "source_commit": "$SOURCE_COMMIT",
  "sha256": "$NORMALIZED_ACTUAL_SHA256",
  "installed_at": "$INSTALLED_AT",
  "os": "$OS",
  "arch": "$ARCH"
}
EOF
# Also expose under current/ for stable path.
if [[ -d "$CURRENT_LINK" || -L "$CURRENT_LINK" ]]; then
  cp -f "$RELEASE_DIR/installed.json" "$CURRENT_LINK/installed.json" 2>/dev/null || true
fi

say "Installed AgenTerm $VERSION to $RELEASE_DIR"
say "Commands are available in $BIN_DIR"
say "Install record: $CURRENT_LINK/installed.json (version $RELEASE_VERSION)"
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
  say "Add $BIN_DIR to PATH to use agenterm-cli (includes mux/mcp subcommands) and agenterm-rhai"
fi

if command -v pgrep >/dev/null 2>&1 && pgrep -f '/agenterm( |$)' >/dev/null 2>&1; then
  # G7a: adaptive text when a live server is still on an older binary.
  say "A running AgenTerm process was detected (server may still be the previous version)."
  say "Disk install is $VERSION, but keep-server windows keep using the already-loaded PE."
  say "To enable $VERSION in the UI: close AgenTerm and choose \"stop server and exit\" (not \"keep server running\"), then reopen."
  say "Or: agenterm-cli shutdown   then start AgenTerm again."
  say "Check disk version without opening a window: agenterm --version   (or agenterm-cli --version)"
fi

if [[ "$NO_LAUNCH" != "1" ]]; then
  say "Launching AgenTerm"
  if [[ "$OS" == "macos" ]]; then
    open "$GUI_PATH"
  elif [[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then
    nohup "$GUI_PATH" >/dev/null 2>&1 &
  else
    say "No graphical session detected; launch $GUI_PATH from your desktop session"
  fi
fi
