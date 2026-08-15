#!/usr/bin/env bash
# Black-box harness: prove `tree` is semantically conformant across Linux
# current, loopback SSH, and dedicated loopback VNC on one cut-owned second
# agenterm-con. Observe-only; no screenshots, coordinates, RDP, or actuation.
#
# Usage (from repository root, graphical session env set):
#   ./scripts/cu-linux-cross-tier-tree.sh
#
# Optional env:
#   CU / CON / ABI / OUT_JSON / WORK_DIR / RFB_PORT / SSH_PORT / MAX_ATTEMPTS
#
# Writes machine-readable evidence to live/348-cross-tier-tree.json by default.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Hard-ban inherited leaks.
unset VNC_PORT AGENTERM_CU_GRANT CARGO_TARGET_DIR \
  AGENTERM_CU_VNC AGENTERM_CU_VNC_PORT AGENTERM_CU_VNC_ENV AGENTERM_CU_VNC_CU \
  AGENTERM_CU_SSH AGENTERM_CU_SSH_PORT AGENTERM_CU_SSH_IDENTITY AGENTERM_CU_SSH_CU \
  AGENTERM_CU_SSH_ENV AGENTERM_CU_SSH_KNOWN_HOSTS AGENTERM_CU_SSH_STRICT_HOSTKEY \
  AGENTERM_ABI_LIB 2>/dev/null || true

export DISPLAY="${DISPLAY:-:2}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp/xdg-runtime-2}"
export DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-unix:path=/tmp/dbus-VaUsohcDxF}"
export AT_SPI_BUS="${AT_SPI_BUS:-unix:path=/tmp/xdg-runtime-2/at-spi/bus_2}"
export AT_SPI_BUS_ADDRESS="${AT_SPI_BUS_ADDRESS:-$AT_SPI_BUS}"

CUT="348"
WORK_DIR="${WORK_DIR:-/tmp/348-cross-tier}"
SSH_DIR="$WORK_DIR/ssh"
VNC_DIR="$WORK_DIR/vnc"
OUT_JSON="${OUT_JSON:-$ROOT/live/348-cross-tier-tree.json}"
EVID_DIR="$(dirname "$OUT_JSON")"
mkdir -p "$WORK_DIR" "$SSH_DIR" "$VNC_DIR" "$EVID_DIR" /tmp/run-box
chmod 700 "$SSH_DIR" 2>/dev/null || true

CU="${CU:-$ROOT/target/cut348/abi-dev/agenterm-cu}"
if [[ ! -x "$CU" ]]; then
  if [[ -x /workspace/agenterm-347/target/cut347/debug/agenterm-cu ]]; then
    CU=/workspace/agenterm-347/target/cut347/debug/agenterm-cu
  elif [[ -x "$ROOT/target/abi-dev/agenterm-cu" ]]; then
    CU="$ROOT/target/abi-dev/agenterm-cu"
  fi
fi
CON="${CON:-/workspace/agenterm-332/target/cut332/debug/agenterm-con}"
ABI="${ABI:-/workspace/agenterm-334/target/cut334/abi-dev}"
export LD_LIBRARY_PATH="$ABI${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export AGENTERM_ABI_LIB="$ABI/libagenterm.so"

RFB_PORT="${RFB_PORT:-5948}"
SSH_PORT="${SSH_PORT:-2348}"
SSH_DEST="${SSH_DEST:-box@127.0.0.1}"
MAX_ATTEMPTS="${MAX_ATTEMPTS:-3}"
SOCK_PATH="/tmp/run-box/agenterm-con-348.sock"
SOCK="unix:$SOCK_PATH"
RESIDENT_SOCK="/tmp/run-box/agenterm-con.sock"
AVATARS=(62399 62403)
FORBIDDEN_X11=(1855 3154 13602 17819)

sshd_pid=""
x11vnc_pid=""
con_pid=""
box_was_locked=0

utc_now() { date -u +%Y-%m-%dT%H:%M:%SZ; }

alive() { [[ -d "/proc/$1" ]]; }

cmd_of() {
  tr '\0' ' ' <"/proc/$1/cmdline" 2>/dev/null || true
}

port_listening() {
  ss -ltn "sport = :$1" 2>/dev/null | grep -q LISTEN
}

sock_identity() {
  python3 - "$1" <<'PY'
import os, sys
st = os.stat(sys.argv[1])
print(f"{st.st_ino} {st.st_size} {int(st.st_mtime)}")
PY
}

kill_only() {
  local pid="$1" label="${2:-proc}"
  [[ -z "$pid" ]] && return 0
  for a in "${AVATARS[@]}" "${FORBIDDEN_X11[@]}"; do
    if [[ "$pid" == "$a" ]]; then
      echo "refusing to signal protected pid $pid ($label)" >&2
      return 1
    fi
  done
  kill -TERM "$pid" 2>/dev/null || sudo kill -TERM "$pid" 2>/dev/null || true
  for _ in $(seq 1 30); do
    alive "$pid" || return 0
    sleep 0.1
  done
  kill -KILL "$pid" 2>/dev/null || sudo kill -KILL "$pid" 2>/dev/null || true
}

cleanup() {
  if [[ -n "${con_pid:-}" ]]; then
    kill_only "$con_pid" "gate-con" || true
  fi
  # Sweep only 348-owned con by socket identity in cmdline.
  for p in /proc/[0-9]*; do
    [[ -r "$p/cmdline" ]] || continue
    pid="${p##*/}"
    for a in "${AVATARS[@]}"; do [[ "$pid" == "$a" ]] && continue 2; done
    cmd="$(tr '\0' ' ' <"$p/cmdline" 2>/dev/null || true)"
    if [[ "$cmd" == *agenterm-con-348.sock* && "$cmd" == *agenterm-con* ]]; then
      kill_only "$pid" "gate-con-sweep" || true
    fi
  done
  if [[ -n "${x11vnc_pid:-}" ]]; then
    kill_only "$x11vnc_pid" "gate-x11vnc" || true
  fi
  for p in /proc/[0-9]*; do
    [[ -r "$p/cmdline" ]] || continue
    pid="${p##*/}"
    for a in "${AVATARS[@]}" "${FORBIDDEN_X11[@]}"; do [[ "$pid" == "$a" ]] && continue 2; done
    cmd="$(tr '\0' ' ' <"$p/cmdline" 2>/dev/null || true)"
    if [[ "$cmd" == *x11vnc* && "$cmd" == *"-rfbport $RFB_PORT"* ]]; then
      kill_only "$pid" "gate-x11vnc-sweep" || true
    fi
  done
  if [[ -f "$SSH_DIR/sshd.pid" ]]; then
    spid="$(tr -d ' \n' <"$SSH_DIR/sshd.pid" || true)"
    if [[ -n "$spid" ]]; then
      sudo kill -TERM "$spid" 2>/dev/null || true
      for _ in $(seq 1 20); do alive "$spid" || break; sleep 0.1; done
      alive "$spid" && sudo kill -KILL "$spid" 2>/dev/null || true
    fi
  fi
  for p in /proc/[0-9]*; do
    [[ -r "$p/cmdline" ]] || continue
    pid="${p##*/}"
    cmd="$(tr '\0' ' ' <"$p/cmdline" 2>/dev/null || true)"
    if [[ "$cmd" == *348-cross-tier/ssh/sshd_config* ]]; then
      sudo kill -TERM "$pid" 2>/dev/null || true
    fi
  done
  if [[ "$box_was_locked" == "1" ]]; then
    sudo usermod -L box 2>/dev/null || true
    box_was_locked=0
  fi
  rm -f "$SOCK_PATH" 2>/dev/null || true
}
trap cleanup EXIT

fail() {
  local reason="$1"
  shift || true
  python3 - "$OUT_JSON" "$reason" "$@" <<'PY'
import json, sys, time
out, reason = sys.argv[1], sys.argv[2]
extra = {}
if len(sys.argv) > 3 and sys.argv[3]:
    try:
        extra = json.loads(sys.argv[3])
    except Exception:
        extra = {"detail": sys.argv[3]}
rec = {
    "gate": "cu-linux-cross-tier-tree",
    "cut": "3.48",
    "ok": False,
    "failed_check": reason,
    "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    "worker_json_does_not_count": True,
    "ceo_owns_official_gate": True,
}
rec.update(extra)
open(out, "w").write(json.dumps(rec, indent=2) + "\n")
print(f"FAIL: {reason}", file=sys.stderr)
sys.exit(1)
PY
}

# --- preflight ---
for a in "${AVATARS[@]}"; do
  alive "$a" || fail "avatar_${a}_missing"
done
[[ -x "$CU" ]] || fail "cu_binary_missing" "{\"cu\":\"$CU\"}"
[[ -x "$CON" ]] || fail "con_binary_missing" "{\"con\":\"$CON\"}"
[[ -f "$ABI/libagenterm.so" ]] || fail "abi_missing" "{\"abi\":\"$ABI\"}"
[[ -e "$RESIDENT_SOCK" ]] || fail "resident_sock_missing"
[[ -z "${VNC_PORT:-}" ]] || fail "vnc_port_env_leaked"

if port_listening "$RFB_PORT"; then
  for p in /proc/[0-9]*; do
    pid="${p##*/}"
    for a in "${AVATARS[@]}" "${FORBIDDEN_X11[@]}"; do [[ "$pid" == "$a" ]] && continue 2; done
    cmd="$(tr '\0' ' ' <"$p/cmdline" 2>/dev/null || true)"
    if [[ "$cmd" == *x11vnc* && "$cmd" == *"-rfbport $RFB_PORT"* ]]; then
      kill_only "$pid" "leftover-x11vnc" || true
    fi
  done
  sleep 0.3
  port_listening "$RFB_PORT" && fail "rfb_port_busy" "{\"port\":$RFB_PORT}"
fi

if port_listening "$SSH_PORT"; then
  for p in /proc/[0-9]*; do
    pid="${p##*/}"
    cmd="$(tr '\0' ' ' <"$p/cmdline" 2>/dev/null || true)"
    if [[ "$cmd" == *348-cross-tier/ssh/sshd_config* ]]; then
      sudo kill -TERM "$pid" 2>/dev/null || true
    fi
  done
  sleep 0.3
  port_listening "$SSH_PORT" && fail "ssh_port_busy" "{\"port\":$SSH_PORT}"
fi

rm -f "$SOCK_PATH" 2>/dev/null || true
resident_before="$(sock_identity "$RESIDENT_SOCK")"

BIN_REV="$(python3 - "$CU" <<'PY'
import hashlib, sys
h = hashlib.sha256()
with open(sys.argv[1], "rb") as f:
    for chunk in iter(lambda: f.read(1 << 20), b""):
        h.update(chunk)
print(h.hexdigest()[:16])
PY
)"
GIT_REV="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"

# --- SSH fixture ---
start_sshd() {
  mkdir -p "$SSH_DIR"
  chmod 700 "$SSH_DIR"
  for name in host_ed25519 id_gate; do
    if [[ ! -f "$SSH_DIR/$name" ]]; then
      if [[ -f /tmp/347-ssh/$name ]]; then
        cp -a "/tmp/347-ssh/$name" "/tmp/347-ssh/$name.pub" "$SSH_DIR/" 2>/dev/null || true
      fi
      if [[ ! -f "$SSH_DIR/$name" ]]; then
        ssh-keygen -q -t ed25519 -N "" -f "$SSH_DIR/$name"
      fi
    fi
  done
  chmod 600 "$SSH_DIR/host_ed25519" "$SSH_DIR/id_gate"
  awk '{print $1" "$2}' "$SSH_DIR/id_gate.pub" >"$SSH_DIR/authorized_keys"
  chmod 600 "$SSH_DIR/authorized_keys"
  hostpub=($(awk '{print $1" "$2}' "$SSH_DIR/host_ed25519.pub"))
  printf '[127.0.0.1]:%s %s %s\n' "$SSH_PORT" "${hostpub[0]}" "${hostpub[1]}" >"$SSH_DIR/known_hosts"
  cat >"$SSH_DIR/sshd_config" <<EOF
Port $SSH_PORT
ListenAddress 127.0.0.1
HostKey $SSH_DIR/host_ed25519
PidFile $SSH_DIR/sshd.pid
AuthorizedKeysFile $SSH_DIR/authorized_keys
StrictModes no
PubkeyAuthentication yes
PasswordAuthentication no
KbdInteractiveAuthentication no
PermitRootLogin no
AllowUsers box
UsePAM no
X11Forwarding no
AllowTcpForwarding yes
LogLevel VERBOSE
SetEnv DISPLAY=$DISPLAY XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS AT_SPI_BUS=$AT_SPI_BUS AT_SPI_BUS_ADDRESS=$AT_SPI_BUS_ADDRESS LD_LIBRARY_PATH=$ABI AGENTERM_ABI_LIB=$ABI/libagenterm.so
AcceptEnv AGENTERM_*
EOF
  cat >"$SSH_DIR/ssh_config" <<EOF
Host gate348
  IdentityFile $SSH_DIR/id_gate
  HostName 127.0.0.1
  Port $SSH_PORT
  User box
  IdentitiesOnly yes
  UserKnownHostsFile $SSH_DIR/known_hosts
  StrictHostKeyChecking yes
  BatchMode yes
  ConnectTimeout 10
EOF
  cat >"$SSH_DIR/remote-cu.sh" <<EOF
#!/bin/sh
echo "\$(date -u +%Y-%m-%dT%H:%M:%SZ) \$\$ \$*" >> $SSH_DIR/remote-cu.invocations
export LD_LIBRARY_PATH=$ABI
export AGENTERM_ABI_LIB=$ABI/libagenterm.so
exec $CU "\$@"
EOF
  chmod 755 "$SSH_DIR/remote-cu.sh"
  : >"$SSH_DIR/remote-cu.invocations"
  sudo mkdir -p /run/sshd 2>/dev/null || true
  st="$(passwd -S box 2>/dev/null || true)"
  if echo " $st " | grep -q ' L '; then
    box_was_locked=1
    sudo usermod -p '*' box
  fi
  sudo /usr/sbin/sshd -f "$SSH_DIR/sshd_config" -E "$SSH_DIR/sshd.log"
  for _ in $(seq 1 30); do
    if [[ -f "$SSH_DIR/sshd.pid" ]] && port_listening "$SSH_PORT"; then
      break
    fi
    sleep 0.1
  done
  [[ -f "$SSH_DIR/sshd.pid" ]] && port_listening "$SSH_PORT" || fail "sshd_failed"
  sshd_pid="$(tr -d ' \n' <"$SSH_DIR/sshd.pid")"
  ssh -F "$SSH_DIR/ssh_config" gate348 true || fail "ssh_smoke_failed"
  sudo chmod 644 "$SSH_DIR/sshd.log" 2>/dev/null || true
}

# --- VNC fixture ---
start_x11vnc() {
  mkdir -p "$VNC_DIR"
  cat >"$VNC_DIR/session-cu.sh" <<EOF
#!/bin/sh
echo "\$(date -u +%Y-%m-%dT%H:%M:%SZ) \$\$ \$*" >> $VNC_DIR/session-cu.invocations
export LD_LIBRARY_PATH=$ABI
export AGENTERM_ABI_LIB=$ABI/libagenterm.so
exec $CU "\$@"
EOF
  chmod 755 "$VNC_DIR/session-cu.sh"
  : >"$VNC_DIR/session-cu.invocations"
  /usr/bin/x11vnc \
    -display "$DISPLAY" \
    -localhost \
    -nopw \
    -shared \
    -forever \
    -noxdamage \
    -rfbport "$RFB_PORT" \
    >"$VNC_DIR/x11vnc.log" 2>&1 &
  x11vnc_pid=$!
  echo "$x11vnc_pid" >"$VNC_DIR/x11vnc.pid"
  for a in "${AVATARS[@]}" "${FORBIDDEN_X11[@]}"; do
    [[ "$x11vnc_pid" == "$a" ]] && fail "x11vnc_pid_collision"
  done
  for _ in $(seq 1 40); do
    if alive "$x11vnc_pid" && port_listening "$RFB_PORT"; then
      return 0
    fi
    sleep 0.15
  done
  fail "x11vnc_failed" "{\"log\":$(python3 -c "print(repr(open('$VNC_DIR/x11vnc.log').read()[-1500:]))")}"
}

# --- second con ---
start_con() {
  local title_boot="348crosstierboot$$"
  rm -f "$WORK_DIR/final-title"
  cat >"$WORK_DIR/inner.sh" <<'INNER'
#!/bin/bash
printf '\033]0;%s\007' "$1"
for i in $(seq 1 80); do
  if [ -f /tmp/348-cross-tier/final-title ]; then
    T=$(python3 -c 'from pathlib import Path; print(Path("/tmp/348-cross-tier/final-title").read_text().strip())')
    printf '\033]0;%s\007' "$T"
    break
  fi
  sleep 0.1
done
exec bash --noprofile --norc
INNER
  chmod 755 "$WORK_DIR/inner.sh"
  "$CON" \
    --no-activate \
    --control "$SOCK" \
    --working-dir "$WORK_DIR" \
    -e bash --noprofile --norc "$WORK_DIR/inner.sh" "$title_boot" \
    >"$WORK_DIR/con.out" 2>&1 &
  con_pid=$!
  echo "$con_pid" >"$WORK_DIR/con.pid"
  for a in "${AVATARS[@]}"; do
    [[ "$con_pid" == "$a" ]] && fail "refusing_avatar_as_con"
  done
  local title="348crosstier${con_pid}"
  echo "$title" >"$WORK_DIR/final-title"
  CON_TITLE="$title"
}

run_cu() {
  # run_cu <out-stem> <timeout> -- argv...
  local stem="$1" timeout="$2"
  shift 2
  local out="$EVID_DIR/${stem}.json" err="$EVID_DIR/${stem}.err"
  local env_extra=()
  # Known hosts for ssh path via env (matches prior gates).
  if [[ "$*" == *"--ssh"* ]]; then
    env_extra+=(
      env
      "AGENTERM_CU_SSH_KNOWN_HOSTS=$SSH_DIR/known_hosts"
      "AGENTERM_CU_SSH_STRICT_HOSTKEY=yes"
      "DISPLAY=$DISPLAY"
      "XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR"
      "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS"
      "AT_SPI_BUS=$AT_SPI_BUS"
      "AT_SPI_BUS_ADDRESS=$AT_SPI_BUS_ADDRESS"
      "LD_LIBRARY_PATH=$ABI"
      "AGENTERM_ABI_LIB=$ABI/libagenterm.so"
    )
  else
    env_extra+=(
      env
      "DISPLAY=$DISPLAY"
      "XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR"
      "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS"
      "AT_SPI_BUS=$AT_SPI_BUS"
      "AT_SPI_BUS_ADDRESS=$AT_SPI_BUS_ADDRESS"
      "LD_LIBRARY_PATH=$ABI"
      "AGENTERM_ABI_LIB=$ABI/libagenterm.so"
    )
  fi
  set +e
  "${env_extra[@]}" timeout "$timeout" "$@" >"$out" 2>"$err"
  local rc=$?
  set -e
  echo "$rc" >"$EVID_DIR/${stem}.exit"
  echo "$out"
}

current_tree_argv() {
  echo "$CU" --target current --grant observe tree --window "$HANDLE"
}

ssh_tree_argv() {
  echo "$CU" --ssh "$SSH_DEST" --ssh-port "$SSH_PORT" \
    --ssh-identity "$SSH_DIR/id_gate" --ssh-cu "$SSH_DIR/remote-cu.sh" \
    --ssh-env "LD_LIBRARY_PATH=$ABI" --ssh-env "AGENTERM_ABI_LIB=$ABI/libagenterm.so" \
    --grant observe tree --window "$HANDLE"
}

vnc_tree_argv() {
  local a=("$CU" --vnc "127.0.0.1:$RFB_PORT" --vnc-cu "$VNC_DIR/session-cu.sh")
  local pairs=(
    "DISPLAY=$DISPLAY"
    "XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR"
    "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS"
    "AT_SPI_BUS=$AT_SPI_BUS"
    "AT_SPI_BUS_ADDRESS=$AT_SPI_BUS_ADDRESS"
    "LD_LIBRARY_PATH=$ABI"
    "AGENTERM_ABI_LIB=$ABI/libagenterm.so"
  )
  for p in "${pairs[@]}"; do
    a+=(--vnc-env "$p")
  done
  a+=(--grant observe tree --window "$HANDLE")
  printf '%q ' "${a[@]}"
}

resolve_handle() {
  HANDLE=""
  local i out
  for i in $(seq 1 40); do
    alive "$con_pid" || fail "gate_con_died" "{\"out\":$(python3 -c "print(repr(open('$WORK_DIR/con.out').read()[-1500:]))")}"
    out="$(run_cu "348-resolve-windows" 25 \
      "$CU" --target current --grant observe windows)"
    HANDLE="$(python3 - "$out" "$CON_TITLE" "$con_pid" <<'PY'
import json, sys
path, title, pid = sys.argv[1], sys.argv[2], int(sys.argv[3])
try:
    payload = json.load(open(path))
except Exception:
    sys.exit(0)
if not payload.get("ok"):
    sys.exit(0)
hits = [
    w for w in (payload.get("data") or [])
    if title in (w.get("title") or "") and w.get("process_id") == pid
]
if len(hits) == 1:
    print(hits[0]["handle"])
PY
)"
    if [[ -n "$HANDLE" ]]; then
      return 0
    fi
    sleep 0.5
  done
  fail "second_con_window_not_unique" "{\"title\":\"$CON_TITLE\",\"con_pid\":$con_pid}"
}

# Semantic normalize + compare (Python owns the comparator).
compare_trees() {
  python3 - "$1" "$2" "$3" "$HANDLE" <<'PY'
import json, sys
from copy import deepcopy

paths = {"current": sys.argv[1], "ssh": sys.argv[2], "vnc": sys.argv[3]}
handle = int(sys.argv[4]) if str(sys.argv[4]).lstrip("-").isdigit() else sys.argv[4]
expected_target = {"current": "current", "ssh": "ssh", "vnc": "vnc"}

VOLATILE_STATES = {"focused", "active", "armed", "selected"}

def load(p):
    with open(p) as f:
        return json.load(f)

def showing(node):
    st = node.get("states") or []
    return isinstance(st, list) and ("showing" in st or "visible" in st)

def norm_states(states):
    if not isinstance(states, list):
        return []
    return sorted(s for s in states if s not in VOLATILE_STATES)

def norm_actions(actions):
    if not isinstance(actions, list):
        return []
    return sorted(str(a) for a in actions)

def node_core(n):
    return {
        "id": n.get("id"),
        "parent_id": n.get("parent_id"),
        "role": n.get("role"),
        "name": n.get("name"),
        "text": n.get("text"),
        "actions": norm_actions(n.get("actions")),
        "states": norm_states(n.get("states")),
        "bounds": n.get("bounds"),
    }

def extract(payload, tier):
    errs = []
    if not isinstance(payload, dict):
        return None, ["not_object"]
    if payload.get("ok") is not True:
        return None, [f"ok_not_true:{payload.get('ok')}", payload.get("error")]
    if payload.get("command") != "tree":
        errs.append(f"command:{payload.get('command')}")
    if payload.get("target") != expected_target[tier]:
        errs.append(f"target:{payload.get('target')}")
    data = payload.get("data") if isinstance(payload.get("data"), dict) else {}
    if data.get("backend") != "at-spi2":
        errs.append(f"backend:{data.get('backend')}")
    if data.get("addressing") != "accessibility-tree":
        errs.append(f"addressing:{data.get('addressing')}")
    win = data.get("window")
    if win != handle and str(win) != str(handle):
        errs.append(f"window:{win}")
    nodes = data.get("nodes")
    if not isinstance(nodes, list) or not nodes:
        errs.append("nodes_missing")
        return None, errs
    # Named showing uniqueness
    counts = {"Command": 0, "SEND": 0, "OffscreenField": 0}
    by_name = {}
    for n in nodes:
        if not isinstance(n, dict) or not showing(n):
            continue
        name = n.get("name")
        if name in counts:
            counts[name] += 1
            by_name[name] = n
    for name, c in counts.items():
        if c != 1:
            errs.append(f"named_{name}_count_{c}")
    if errs:
        return None, errs
    # Stable path graph by node id
    cores = [node_core(n) for n in nodes if isinstance(n, dict)]
    cores_by_id = {c["id"]: c for c in cores if c.get("id") is not None}
    # Drop focused-only volatility already in states; keep structure.
    semantic = {
        "backend": data.get("backend"),
        "addressing": data.get("addressing"),
        "mechanism": data.get("mechanism"),
        "window": data.get("window"),
        "root_id": data.get("root_id"),
        "node_count": len(nodes),
        "nodes_by_id": cores_by_id,
        "named": {
            k: {
                "role": by_name[k].get("role"),
                "name": by_name[k].get("name"),
                "text": by_name[k].get("text"),
                "actions": norm_actions(by_name[k].get("actions")),
                "states": norm_states(by_name[k].get("states")),
                "parent_id": by_name[k].get("parent_id"),
                "id": by_name[k].get("id"),
                "bounds": by_name[k].get("bounds"),
            }
            for k in ("Command", "SEND", "OffscreenField")
        },
    }
    return semantic, errs

raw = {t: load(p) for t, p in paths.items()}
sem = {}
errs = {}
for t in ("current", "ssh", "vnc"):
    s, e = extract(raw[t], t)
    sem[t] = s
    errs[t] = e

if any(sem[t] is None for t in sem):
    print(json.dumps({
        "ok": False,
        "error": "cross_tier_conformance_failed",
        "reason": "tier_extract_failed",
        "errs": errs,
        "envelope": {
            t: {
                "ok": raw[t].get("ok") if isinstance(raw[t], dict) else None,
                "target": raw[t].get("target") if isinstance(raw[t], dict) else None,
                "command": raw[t].get("command") if isinstance(raw[t], dict) else None,
                "error": raw[t].get("error") if isinstance(raw[t], dict) else None,
            }
            for t in raw
        },
    }))
    sys.exit(2)

# Compare semantic cores: root, count, named nodes, full node graph excluding
# only volatile focus/active states (already stripped). Bounds must match.
base = sem["current"]
mismatches = []

def cmp_field(path, a, b):
    if a != b:
        mismatches.append({"path": path, "current": a, "other": b})

for tier in ("ssh", "vnc"):
    other = sem[tier]
    cmp_field(f"{tier}.root_id", base["root_id"], other["root_id"])
    cmp_field(f"{tier}.node_count", base["node_count"], other["node_count"])
    cmp_field(f"{tier}.backend", base["backend"], other["backend"])
    cmp_field(f"{tier}.mechanism", base["mechanism"], other["mechanism"])
    # Named nodes
    for name in ("Command", "SEND", "OffscreenField"):
        for field in ("role", "name", "text", "actions", "states", "parent_id", "id", "bounds"):
            cmp_field(
                f"{tier}.named.{name}.{field}",
                base["named"][name].get(field),
                other["named"][name].get(field),
            )
    # Full node set by id
    if set(base["nodes_by_id"]) != set(other["nodes_by_id"]):
        mismatches.append({
            "path": f"{tier}.node_ids",
            "current": sorted(base["nodes_by_id"]),
            "other": sorted(other["nodes_by_id"]),
        })
    else:
        for nid, cn in base["nodes_by_id"].items():
            on = other["nodes_by_id"][nid]
            for field in ("parent_id", "role", "name", "text", "actions", "states", "bounds"):
                cmp_field(f"{tier}.node[{nid}].{field}", cn.get(field), on.get(field))

# Document focus-state difference allowance: already stripped from states.
focus_note = "volatile states focused/active/armed/selected excluded from compare"

result = {
    "ok": len(mismatches) == 0,
    "error": None if not mismatches else "cross_tier_conformance_failed",
    "mismatches": mismatches,
    "focus_state_policy": focus_note,
    "semantic": {
        t: {
            "root_id": sem[t]["root_id"],
            "node_count": sem[t]["node_count"],
            "named": sem[t]["named"],
        }
        for t in sem
    },
    "envelope": {
        t: {
            "ok": raw[t].get("ok"),
            "target": raw[t].get("target"),
            "command": raw[t].get("command"),
            "backend": (raw[t].get("data") or {}).get("backend") if isinstance(raw[t].get("data"), dict) else None,
            "window": (raw[t].get("data") or {}).get("window") if isinstance(raw[t].get("data"), dict) else None,
        }
        for t in raw
    },
}
print(json.dumps(result, indent=2))
sys.exit(0 if result["ok"] else 3)
PY
}

# capabilities preflight (not the cut; declaration only)
preflight_caps() {
  local out
  out="$(run_cu "348-cap-current" 20 \
    "$CU" --target current --grant observe capabilities)"
  python3 - "$out" <<'PY' || fail "cap_current_tree_not_declared"
import json, sys
p = json.load(open(sys.argv[1]))
assert p.get("ok") is True and p.get("target") == "current"
verbs = (p.get("data") or {}).get("verbs") or {}
tree = verbs.get("tree") or {}
assert tree.get("status") == "available", tree
print("current tree available")
PY
  out="$(run_cu "348-cap-ssh" 30 \
    "$CU" --ssh "$SSH_DEST" --ssh-port "$SSH_PORT" \
    --ssh-identity "$SSH_DIR/id_gate" --ssh-cu "$SSH_DIR/remote-cu.sh" \
    --ssh-env "LD_LIBRARY_PATH=$ABI" --ssh-env "AGENTERM_ABI_LIB=$ABI/libagenterm.so" \
    --grant observe capabilities)"
  python3 - "$out" <<'PY' || fail "cap_ssh_tree_not_declared"
import json, sys
p = json.load(open(sys.argv[1]))
assert p.get("ok") is True and p.get("target") == "ssh"
d = p.get("data") or {}
assert d.get("target") == "ssh"
verbs = d.get("verbs") or {}
tree = verbs.get("tree") or {}
assert tree.get("status") == "available", tree
print("ssh tree available")
PY
  local vargs=("$CU" --vnc "127.0.0.1:$RFB_PORT" --vnc-cu "$VNC_DIR/session-cu.sh")
  for p in \
    "DISPLAY=$DISPLAY" \
    "XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR" \
    "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS" \
    "AT_SPI_BUS=$AT_SPI_BUS" \
    "AT_SPI_BUS_ADDRESS=$AT_SPI_BUS_ADDRESS" \
    "LD_LIBRARY_PATH=$ABI" \
    "AGENTERM_ABI_LIB=$ABI/libagenterm.so"
  do
    vargs+=(--vnc-env "$p")
  done
  vargs+=(--grant observe capabilities)
  out="$(run_cu "348-cap-vnc" 30 "${vargs[@]}")"
  python3 - "$out" <<'PY' || fail "cap_vnc_tree_not_declared"
import json, sys
p = json.load(open(sys.argv[1]))
assert p.get("ok") is True and p.get("target") == "vnc"
d = p.get("data") or {}
assert d.get("target") == "vnc"
verbs = d.get("verbs") or {}
tree = verbs.get("tree") or {}
assert tree.get("status") == "available", tree
print("vnc tree available")
PY
  out="$(run_cu "348-cap-rdp" 10 \
    "$CU" --rdp "127.0.0.1:13389" --grant observe capabilities)"
  python3 - "$out" <<'PY' || fail "cap_rdp_tree_wrongly_supported"
import json, sys
p = json.load(open(sys.argv[1]))
assert p.get("ok") is True and p.get("target") == "rdp"
d = p.get("data") or {}
verbs = d.get("verbs") or {}
tree = verbs.get("tree") or {}
assert tree.get("status") in ("unsupported", "unavailable"), tree
assert tree.get("reason") == "rdp_unavailable", tree
print("rdp tree unsupported")
PY
}

# --- main ---
echo "== 3.48 cross-tier tree harness =="
echo "CU=$CU"
echo "CON=$CON"
echo "ABI=$ABI"
echo "RFB_PORT=$RFB_PORT SSH_PORT=$SSH_PORT"
echo "OUT=$OUT_JSON"

start_sshd
start_x11vnc
start_con
resolve_handle
echo "HANDLE=$HANDLE title=$CON_TITLE con_pid=$con_pid"

preflight_caps

attempt=1
compare_json=""
while [[ "$attempt" -le "$MAX_ATTEMPTS" ]]; do
  echo "-- attempt $attempt/$MAX_ATTEMPTS --"
  cur_out="$(run_cu "348-tree-current-a${attempt}" 40 \
    $CU --target current --grant observe tree --window "$HANDLE")"
  ssh_out="$(run_cu "348-tree-ssh-a${attempt}" 45 \
    $CU --ssh "$SSH_DEST" --ssh-port "$SSH_PORT" \
    --ssh-identity "$SSH_DIR/id_gate" --ssh-cu "$SSH_DIR/remote-cu.sh" \
    --ssh-env "LD_LIBRARY_PATH=$ABI" --ssh-env "AGENTERM_ABI_LIB=$ABI/libagenterm.so" \
    --grant observe tree --window "$HANDLE")"
  vargs=("$CU" --vnc "127.0.0.1:$RFB_PORT" --vnc-cu "$VNC_DIR/session-cu.sh")
  for p in \
    "DISPLAY=$DISPLAY" \
    "XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR" \
    "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS" \
    "AT_SPI_BUS=$AT_SPI_BUS" \
    "AT_SPI_BUS_ADDRESS=$AT_SPI_BUS_ADDRESS" \
    "LD_LIBRARY_PATH=$ABI" \
    "AGENTERM_ABI_LIB=$ABI/libagenterm.so"
  do
    vargs+=(--vnc-env "$p")
  done
  vargs+=(--grant observe tree --window "$HANDLE")
  vnc_out="$(run_cu "348-tree-vnc-a${attempt}" 45 "${vargs[@]}")"

  set +e
  compare_json="$(compare_trees "$cur_out" "$ssh_out" "$vnc_out")"
  crc=$?
  set -e
  echo "$compare_json" >"$EVID_DIR/348-compare-a${attempt}.json"
  if [[ "$crc" -eq 0 ]]; then
    echo "PASS semantic compare on attempt $attempt"
    break
  fi
  echo "mismatch on attempt $attempt (rc=$crc); retrying full set if budget remains" >&2
  attempt=$((attempt + 1))
  if [[ "$attempt" -le "$MAX_ATTEMPTS" ]]; then
    sleep 0.4
  fi
done

resident_after="$(sock_identity "$RESIDENT_SOCK")"
avatars_after="$(python3 - <<PY
import json
from pathlib import Path
out={}
for p in (62399, 62403):
    out[str(p)] = Path(f"/proc/{p}").exists()
print(json.dumps(out))
PY
)"

python3 - "$OUT_JSON" <<PY
import json, os, time
from pathlib import Path

out = Path("$OUT_JSON")
compare_path = Path("$EVID_DIR/348-compare-a${attempt}.json") if $attempt <= $MAX_ATTEMPTS else Path("$EVID_DIR/348-compare-a$((MAX_ATTEMPTS)).json")
# pick last successful or last attempt
cands = sorted(Path("$EVID_DIR").glob("348-compare-a*.json"))
compare = {}
ok = False
attempt_used = $attempt if $attempt <= $MAX_ATTEMPTS else $MAX_ATTEMPTS
for p in cands:
    try:
        obj = json.loads(p.read_text())
    except Exception:
        continue
    compare = obj
    if obj.get("ok") is True:
        ok = True
        attempt_used = int(p.stem.split("a")[-1])
        break
if not ok and cands:
    compare = json.loads(cands[-1].read_text())
    attempt_used = int(cands[-1].stem.split("a")[-1])

raw_paths = {
    "current": str(Path("$EVID_DIR") / f"348-tree-current-a{attempt_used}.json"),
    "ssh": str(Path("$EVID_DIR") / f"348-tree-ssh-a{attempt_used}.json"),
    "vnc": str(Path("$EVID_DIR") / f"348-tree-vnc-a{attempt_used}.json"),
}

rec = {
    "gate": "cu-linux-cross-tier-tree",
    "cut": "3.48",
    "ok": ok and compare.get("ok") is True,
    "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    "runner": "scripts/cu-linux-cross-tier-tree.sh",
    "worker_json_does_not_count": True,
    "ceo_owns_official_gate": True,
    "binary": {
        "cu": "$CU",
        "sha256_16": "$BIN_REV",
        "git_rev": "$GIT_REV",
        "con": "$CON",
        "abi": "$ABI",
    },
    "endpoint_ownership": {
        "sshd_pid": "$(cat "$SSH_DIR/sshd.pid" 2>/dev/null || true)",
        "ssh_port": $SSH_PORT,
        "x11vnc_pid": $(cat "$VNC_DIR/x11vnc.pid" 2>/dev/null || echo null),
        "rfb_port": $RFB_PORT,
        "dedicated_x11vnc": True,
        "not_resident_5902_only": True,
        "control_socket": "$SOCK",
        "con_pid": $con_pid,
        "title": "$CON_TITLE",
        "handle": $HANDLE,
        "same_fixture_for_all_tiers": True,
    },
    "avatars": {
        "protected": [62399, 62403],
        "after_alive": json.loads('''$avatars_after'''),
    },
    "resident_sock": {
        "path": "$RESIDENT_SOCK",
        "before": "$resident_before",
        "after": "$resident_after",
        "unchanged": "$resident_before" == "$resident_after",
    },
    "raw_reply_paths": raw_paths,
    "normalized_comparison": compare,
    "attempt_used": attempt_used,
    "max_attempts": $MAX_ATTEMPTS,
    "preflight_capabilities": {
        "current": str(Path("$EVID_DIR") / "348-cap-current.json"),
        "ssh": str(Path("$EVID_DIR") / "348-cap-ssh.json"),
        "vnc": str(Path("$EVID_DIR") / "348-cap-vnc.json"),
        "rdp": str(Path("$EVID_DIR") / "348-cap-rdp.json"),
    },
    "cleanup": {
        "trap_installed": True,
        "note": "EXIT trap kills only cut-owned con/x11vnc/sshd by recorded identity",
    },
}
out.write_text(json.dumps(rec, indent=2) + "\n")
print(json.dumps({"ok": rec["ok"], "out": str(out), "attempt_used": attempt_used}, indent=2))
if not rec["ok"]:
    raise SystemExit(1)
PY

echo "PASS: live/348-cross-tier-tree.json written"
# Also mirror under ceo/live for operator visibility (does not count as official gate).
if [[ -d /workspace/ceo/live ]]; then
  cp -f "$OUT_JSON" /workspace/ceo/live/348-cross-tier-tree.json 2>/dev/null || true
fi
