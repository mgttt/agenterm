#!/bin/sh
# generate-pc.sh -- generate libagenterm.pc from libagenterm.pc.in (milestone 52).
#
# The packaging script the .pc.in template was waiting for. Replaces every
# @PLACEHOLDER@ in libagenterm.pc.in with the values given on the command
# line, writes the result to --output, and then verifies that no @...@
# placeholder survived (a leftover means a consumer would silently link with
# an empty/raw field, so it is a hard error, not a warning).
#
# Anti-drift gate note: the two SYSTEM_LIBS_* assignments below are the parse
# anchor for crates/agenterm-abi/tests/pkgconfig_libs.rs (the three-way gate
# README table <-> tests/common/mod.rs::system_libs <-> this script). Keep the
# exact single-line `SYSTEM_LIBS_<OS>='...'` form; the gate splits the quoted
# value on whitespace and compares it token-for-token with
# common::system_libs::LINUX / MACOS.

set -eu

# Per-platform system libraries a STATIC consumer must also link (pkg-config
# `Libs.private`). Measured values; the single source of truth is
# tests/common/mod.rs::system_libs, mirrored in packaging/pkgconfig/README.md.
SYSTEM_LIBS_LINUX='-ldl -lpthread -lm'
SYSTEM_LIBS_DARWIN='-framework CoreFoundation -framework CoreGraphics -framework AppKit -framework Foundation -framework QuartzCore -framework Metal -framework IOKit -framework Carbon -ldl -lpthread -lm'

usage() {
    cat >&2 <<'EOF'
usage: generate-pc.sh --prefix <dir> --version <ver> --output <file>
                      [--libdir <dir>] [--includedir <dir>]
                      [--system auto|linux|darwin]

  --prefix <dir>      install prefix (required, e.g. /usr/local or a ~/.local prefix)
  --libdir <dir>      dir containing libagenterm.{a,so,dylib}; default: <prefix>/lib
  --includedir <dir>  dir containing agenterm.h;            default: <prefix>/include
  --version <ver>     libagenterm version (required; the agenterm-abi crate version)
  --output <file>     where to write the generated .pc (required)
  --system <os>       force the Libs.private set: linux | darwin | auto (default auto).
                      auto selects on `uname -s` (Linux/Darwin only); any other
                      platform (including MSYS/MINGW on Windows) is an explicit
                      error -- never an empty Libs.private. --system exists for
                      tests and cross packaging; normal installs keep auto.
  -h, --help          show this help and exit

Example (from the repository root):
  sh packaging/pkgconfig/generate-pc.sh \
      --prefix /usr/local --version 0.1.16 \
      --output /usr/local/lib/pkgconfig/libagenterm.pc
EOF
    exit 1
}

PREFIX=
LIBDIR=
INCLUDEDIR=
VERSION=
OUTPUT=
SYSTEM=auto

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)    [ $# -ge 2 ] || usage; PREFIX=$2; shift 2 ;;
        --libdir)    [ $# -ge 2 ] || usage; LIBDIR=$2; shift 2 ;;
        --includedir) [ $# -ge 2 ] || usage; INCLUDEDIR=$2; shift 2 ;;
        --version)   [ $# -ge 2 ] || usage; VERSION=$2; shift 2 ;;
        --output)    [ $# -ge 2 ] || usage; OUTPUT=$2; shift 2 ;;
        --system)    [ $# -ge 2 ] || usage; SYSTEM=$2; shift 2 ;;
        -h|--help)   usage ;;
        *)
            echo "generate-pc.sh: unknown argument: $1" >&2
            usage
            ;;
    esac
done

[ -n "$PREFIX" ] || { echo "generate-pc.sh: --prefix is required" >&2; usage; }
[ -n "$VERSION" ] || { echo "generate-pc.sh: --version is required" >&2; usage; }
[ -n "$OUTPUT" ] || { echo "generate-pc.sh: --output is required" >&2; usage; }

# Sensible defaults: libdir/includedir derive from the prefix when not given.
: "${LIBDIR:=$PREFIX/lib}"
: "${INCLUDEDIR:=$PREFIX/include}"

# Select the Libs.private set. Never silently emit an empty set for an
# unknown platform -- a consumer would then fail to link at build time.
case "$SYSTEM" in
    auto)
        case "$(uname -s)" in
            Linux)  SYSTEM_LIBS=$SYSTEM_LIBS_LINUX ;;
            Darwin) SYSTEM_LIBS=$SYSTEM_LIBS_DARWIN ;;
            *)
                echo "generate-pc.sh: unsupported platform '$(uname -s)' -- refusing to emit an empty Libs.private. This script serves Unix; use --system=linux|darwin only for tests and cross packaging." >&2
                exit 1
                ;;
        esac
        ;;
    linux)  SYSTEM_LIBS=$SYSTEM_LIBS_LINUX ;;
    darwin) SYSTEM_LIBS=$SYSTEM_LIBS_DARWIN ;;
    *)
        echo "generate-pc.sh: unknown --system '$SYSTEM' (expected auto|linux|darwin)" >&2
        exit 1
        ;;
esac

SCRIPT_DIR=$(dirname "$0")
TEMPLATE="${SCRIPT_DIR}/libagenterm.pc.in"
[ -f "$TEMPLATE" ] || {
    echo "generate-pc.sh: template not found: $TEMPLATE" >&2
    exit 1
}
OUT_DIR=$(dirname "$OUTPUT")
[ -d "$OUT_DIR" ] || {
    echo "generate-pc.sh: output directory does not exist: $OUT_DIR" >&2
    exit 1
}

# Escape sed replacement-string metacharacters (&, \ and the | delimiter) so
# user-provided paths/versions cannot corrupt the substitution.
sed_escape() {
    printf '%s' "$1" | sed 's/[&\\|]/\\&/g'
}

TMP="${OUTPUT}.tmp.$$"
# Substitute only in non-comment lines (/^#/!): the .pc.in header comment
# documents the placeholders themselves, so it must survive verbatim instead
# of being rewritten into path soup inside the generated file.
sed \
    -e "/^#/! s|@PREFIX@|$(sed_escape "$PREFIX")|g" \
    -e "/^#/! s|@LIBDIR@|$(sed_escape "$LIBDIR")|g" \
    -e "/^#/! s|@INCLUDEDIR@|$(sed_escape "$INCLUDEDIR")|g" \
    -e "/^#/! s|@VERSION@|$(sed_escape "$VERSION")|g" \
    -e "/^#/! s|@SYSTEM_LIBS@|$(sed_escape "$SYSTEM_LIBS")|g" \
    "$TEMPLATE" > "$TMP"

# A leftover @...@ in any non-comment line means the .pc would carry a raw
# placeholder to consumers -- hard failure, naming exactly which placeholders
# survived.
LEFTOVER=$(grep -v '^#' "$TMP" | grep -o '@[A-Za-z_][A-Za-z0-9_]*@' || true)
if [ -n "$LEFTOVER" ]; then
    echo "generate-pc.sh: unresolved placeholder(s) in generated .pc: $(printf '%s' "$LEFTOVER" | tr '\n' ' ')" >&2
    rm -f "$TMP"
    exit 1
fi

mv "$TMP" "$OUTPUT"
echo "generate-pc.sh: wrote $OUTPUT (Libs.private selected for: $SYSTEM)"
