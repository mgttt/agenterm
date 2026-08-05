#!/usr/bin/env python3
"""Shared Cursor Cloud Agent fleet helpers (registry / mailbox / pulse / duty).

Used by thin bash entrypoints under scripts/cursor_agent_*.sh.
Never prints CURSOR_API. Degrades to git-only when live list is unavailable.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
from base64 import b64encode
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Optional

API_BASE_DEFAULT = "https://api.cursor.com/v1"


@dataclass
class Peer:
    name: str
    bc_id: str
    role: str = ""


@dataclass
class Seat:
    name: str
    bullets: list[str] = field(default_factory=list)
    status: str = ""
    branch: str = ""
    tip: str = ""
    next_step: str = ""
    blocked: str = ""


@dataclass
class LiveAgent:
    status: str = "?"
    branch: str = ""
    updated_at_ms: int = 0


def repo_root() -> Path:
    here = Path(__file__).resolve().parent.parent.parent
    return here


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError:
        return ""


def parse_registry(markdown: str) -> list[Peer]:
    """Parse skills/cursor/session-registry.md peer table.

    Only rows with a real ``bc-…`` id are returned (Automations placeholder
    rows like ``_(每次…)_`` are skipped).
    """
    peers: list[Peer] = []
    for line in markdown.splitlines():
        if not line.strip().startswith("|"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 4:
            continue
        name = re.sub(r"\*+", "", cells[0]).strip()
        bc = cells[1].strip().strip("`")
        role = cells[3] if len(cells) > 3 else ""
        if name in ("显示名", "") or not bc.startswith("bc-"):
            continue
        peers.append(Peer(name=name, bc_id=bc, role=role))
    return peers


def parse_shared_facts(markdown: str, limit: int = 6) -> list[str]:
    shared: list[str] = []
    in_shared = False
    for line in markdown.splitlines():
        if line.startswith("## 共享事实"):
            in_shared = True
            continue
        if in_shared:
            if line.startswith("## "):
                break
            if line.strip().startswith("|") and "---" not in line:
                cells = [c.strip() for c in line.strip().strip("|").split("|")]
                if len(cells) >= 2 and cells[0] not in ("键", ""):
                    shared.append(f"{cells[0]}={cells[1]}")
    return shared[:limit]


def parse_seats(markdown: str) -> dict[str, Seat]:
    """Parse mailbox seat blocks. Keys are exact display names (no substring)."""
    seats: dict[str, Seat] = {}
    current: Optional[str] = None
    for line in markdown.splitlines():
        match = re.match(r"^###\s+(.+?)\s*·", line)
        if match:
            current = match.group(1).strip()
            seats[current] = Seat(name=current)
            continue
        if current is None or not line.startswith("- "):
            continue
        item = line[2:].strip()
        seat = seats[current]
        seat.bullets.append(item)
        if item.startswith("状态:"):
            seat.status = item.split(":", 1)[1].strip()
        elif item.startswith("分支:"):
            seat.branch = item.split(":", 1)[1].strip().strip("`")
        elif item.startswith("tip:") or item.startswith("证据:"):
            seat.tip = item
        elif item.startswith("下一步:"):
            seat.next_step = item.split(":", 1)[1].strip()
        elif item.startswith("阻塞") or item.startswith("阻塞/请示:"):
            seat.blocked = item.split(":", 1)[1].strip() if ":" in item else item
    return seats


def parse_open_asks(markdown: str) -> list[str]:
    asks: list[str] = []
    for block in re.split(r"(?=### 请示#)", markdown):
        if not block.startswith("### 请示#"):
            continue
        heading = block.splitlines()[0]
        if "已决" in heading:
            continue
        if re.search(r"主控回复:\s*（空着", block) or re.search(
            r"主控回复:\s*$", block, re.M
        ):
            asks.append(heading)
    return asks


def index_live_agents(live_json: str) -> tuple[dict[str, LiveAgent], dict[str, LiveAgent]]:
    by_id: dict[str, LiveAgent] = {}
    by_name: dict[str, LiveAgent] = {}
    if not live_json.strip():
        return by_id, by_name
    try:
        data = json.loads(live_json)
    except json.JSONDecodeError:
        return by_id, by_name
    items = data.get("items") or data.get("agents") or []
    for item in items:
        if not isinstance(item, dict):
            continue
        agent_id = item.get("id") or item.get("agentId") or ""
        name = item.get("name") or ""
        entry = LiveAgent(
            status=item.get("status") or item.get("agentStatus") or "?",
            branch=item.get("branchName") or "",
            updated_at_ms=int(
                item.get("updatedAtMs") or item.get("lastMessageActivityAtMs") or 0
            ),
        )
        if agent_id:
            by_id[str(agent_id)] = entry
        if name:
            by_name[str(name)] = entry
    return by_id, by_name


def fetch_agents_json(
    api_base: str = API_BASE_DEFAULT, timeout: float = 25.0
) -> str:
    key = os.environ.get("CURSOR_API") or ""
    if not key:
        return ""
    url = f"{api_base.rstrip('/')}/agents?limit=100"
    token = b64encode(f"{key}:".encode()).decode()
    request = urllib.request.Request(
        url,
        headers={"Authorization": f"Basic {token}"},
        method="GET",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return response.read().decode("utf-8", errors="replace")
    except (urllib.error.URLError, TimeoutError, OSError):
        return ""


def git_short_sha(root: Path, ref: str = "origin/main") -> str:
    try:
        return subprocess.check_output(
            ["git", "-C", str(root), "rev-parse", "--short", ref],
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        try:
            return subprocess.check_output(
                ["git", "-C", str(root), "rev-parse", "--short", "HEAD"],
                text=True,
                stderr=subprocess.DEVNULL,
            ).strip()
        except (subprocess.CalledProcessError, FileNotFoundError):
            return "?"


def branches_ahead_of_main(root: Path) -> list[tuple[str, str, int]]:
    """Return (branch, tip_sha, ahead_count) for origin/cursor/* ahead of main."""
    subprocess.run(
        ["git", "-C", str(root), "fetch", "origin", "--prune"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    try:
        refs = subprocess.check_output(
            [
                "git",
                "-C",
                str(root),
                "for-each-ref",
                "--format=%(refname:short) %(objectname:short)",
                "refs/remotes/origin/cursor",
            ],
            text=True,
            stderr=subprocess.DEVNULL,
        )
    except (subprocess.CalledProcessError, FileNotFoundError):
        return []
    out: list[tuple[str, str, int]] = []
    for line in refs.splitlines():
        if not line.strip():
            continue
        parts = line.split()
        if len(parts) < 2:
            continue
        ref, sha = parts[0], parts[1]
        branch = ref.removeprefix("origin/")
        try:
            ahead = int(
                subprocess.check_output(
                    ["git", "-C", str(root), "rev-list", "--count", f"origin/main..{ref}"],
                    text=True,
                    stderr=subprocess.DEVNULL,
                ).strip()
            )
        except (subprocess.CalledProcessError, ValueError):
            ahead = 0
        if ahead > 0:
            out.append((branch, sha, ahead))
    return out


def build_fleet_pulse(
    *,
    root: Optional[Path] = None,
    registry_path: Optional[Path] = None,
    mailbox_path: Optional[Path] = None,
    live_json: str = "",
    plain: bool = False,
    fetch_live: bool = True,
    api_base: str = API_BASE_DEFAULT,
) -> str:
    root = root or repo_root()
    registry_path = registry_path or root / "skills/cursor/session-registry.md"
    mailbox_path = mailbox_path or root / "skills/cursor/mailbox.md"
    if fetch_live and not live_json:
        live_json = fetch_agents_json(api_base=api_base)

    peers = parse_registry(read_text(registry_path))
    mailbox = read_text(mailbox_path)
    shared = parse_shared_facts(mailbox)
    seats = parse_seats(mailbox)
    by_id, by_name = index_live_agents(live_json)
    main_sha = git_short_sha(root)

    lines = [
        f"main:{main_sha}",
        f"shared:{'; '.join(shared) if shared else '(none)'}",
        "peers:",
    ]
    if not peers:
        lines.append("- (registry empty or unreadable)")
    for peer in peers:
        live = by_id.get(peer.bc_id) or by_name.get(peer.name)
        live_s = live.status if live else "git-only"
        branch = live.branch if live else ""
        seat = seats.get(peer.name)
        if seat and seat.bullets:
            seat_s = " | ".join(seat.bullets[:4])
        else:
            seat_s = peer.role
        branch_s = f" branch={branch}" if branch else ""
        lines.append(f"- {peer.name} [{live_s}]{branch_s} :: {seat_s}")

    body = "\n".join(lines)
    if plain:
        return body + "\n"
    return f"<fleet-pulse>\n{body}\n</fleet-pulse>\n"


def scan_duty(
    *,
    root: Optional[Path] = None,
    registry_path: Optional[Path] = None,
    mailbox_path: Optional[Path] = None,
    live_json: str = "",
    stale_hours: float = 4.0,
    fetch_live: bool = True,
    api_base: str = API_BASE_DEFAULT,
) -> dict[str, Any]:
    root = root or repo_root()
    registry_path = registry_path or root / "skills/cursor/session-registry.md"
    mailbox_path = mailbox_path or root / "skills/cursor/mailbox.md"
    if fetch_live and not live_json:
        live_json = fetch_agents_json(api_base=api_base)

    peers = parse_registry(read_text(registry_path))
    mailbox = read_text(mailbox_path)
    seats = parse_seats(mailbox)
    by_id, by_name = index_live_agents(live_json)
    findings: list[dict[str, Any]] = []

    for branch, sha, ahead in branches_ahead_of_main(root):
        findings.append(
            {
                "kind": "unmerged_branch",
                "severity": "high",
                "branch": branch,
                "tip": sha,
                "ahead": ahead,
                "summary": (
                    f"branch {branch} @{sha} is {ahead} commit(s) ahead of "
                    "origin/main — review/merge"
                ),
                "nudge": None,
            }
        )

    now_ms = datetime.now(timezone.utc).timestamp() * 1000
    stale_ms = stale_hours * 3600 * 1000

    for peer in peers:
        live = by_id.get(peer.bc_id) or by_name.get(peer.name)
        seat = seats.get(peer.name)
        status_seat = (seat.status if seat else "") or peer.role
        if live and live.updated_at_ms and live.status.upper() in (
            "RUNNING",
            "ACTIVE",
        ):
            if (now_ms - live.updated_at_ms) >= stale_ms:
                age_h = (now_ms - live.updated_at_ms) / 3600000
                findings.append(
                    {
                        "kind": "stale_live",
                        "severity": "medium",
                        "peer": peer.name,
                        "bcId": peer.bc_id,
                        "summary": (
                            f"{peer.name} live={live.status} last activity "
                            f"~{age_h:.1f}h ago — probe"
                        ),
                        "nudge": {
                            "to": peer.name,
                            "text": (
                                f"duty探活：mailbox 见你席位「{status_seat[:80]}」。"
                                "请 pull main、刷新席位心跳；若已交付请 tip SHA，"
                                "若阻塞请写请示。"
                            ),
                        },
                    }
                )

        # Seat-local dependency wait only (avoid matching shared-facts merge text).
        if seat and "等" in seat.status and (
            "设计" in seat.status or "Phase" in seat.status or "依赖" in seat.status
        ):
            findings.append(
                {
                    "kind": "dependency",
                    "severity": "medium",
                    "peer": peer.name,
                    "bcId": peer.bc_id,
                    "summary": f"{peer.name} seat still waiting on dependency — verify unlock",
                    "nudge": {
                        "to": peer.name,
                        "text": (
                            "duty：请 pull main 核对依赖是否已合；可开工则更新席位并继续，"
                            "否则写阻塞。"
                        ),
                    },
                }
            )

    for ask in parse_open_asks(mailbox):
        findings.append(
            {
                "kind": "open_ask",
                "severity": "high",
                "summary": f"unanswered 请示: {ask}",
                "nudge": None,
            }
        )

    controller = next((p for p in peers if "当前主控" in p.role), None)
    return {
        "main": git_short_sha(root),
        "generatedAt": datetime.now(timezone.utc).isoformat(),
        "controller": (
            {"name": controller.name, "bcId": controller.bc_id, "role": controller.role}
            if controller
            else None
        ),
        "findingCount": len(findings),
        "findings": findings,
    }


def format_duty_text(result: dict[str, Any]) -> str:
    lines = [
        f"duty scan main={result.get('main')} findings={result.get('findingCount', 0)}"
    ]
    findings = result.get("findings") or []
    if not findings:
        lines.append("noop: fleet quiet")
    for index, finding in enumerate(findings, 1):
        lines.append(
            f"{index}. [{finding.get('severity')}] {finding.get('kind')}: "
            f"{finding.get('summary')}"
        )
    nudges = [f for f in findings if f.get("nudge")]
    if nudges:
        lines.append(f"nudge-candidates: {len(nudges)}")
    return "\n".join(lines) + "\n"


def apply_nudges(
    result: dict[str, Any],
    *,
    from_name: str,
    root: Optional[Path] = None,
) -> int:
    root = root or repo_root()
    chat = root / "scripts/cursor_agent_chat.sh"
    sent = 0
    seen: set[str] = set()
    for finding in result.get("findings") or []:
        nudge = finding.get("nudge")
        if not nudge:
            continue
        to = nudge["to"]
        if to in seen:
            continue
        seen.add(to)
        proc = subprocess.run(
            [
                str(chat),
                "--from",
                from_name,
                "--to",
                to,
                "--no-wait",
                "--no-fleet-context",
                "--stdin",
            ],
            input=nudge["text"],
            text=True,
            cwd=str(root),
            capture_output=True,
        )
        tail = (proc.stdout or proc.stderr or "")[-160:]
        print(f"nudge {to}: rc={proc.returncode} {tail}", flush=True)
        sent += 1
    print(f"apply done nudges={sent}", flush=True)
    return sent


def _cli_pulse(args: argparse.Namespace) -> int:
    root = Path(args.root) if args.root else repo_root()
    text = build_fleet_pulse(
        root=root,
        registry_path=Path(args.registry) if args.registry else None,
        mailbox_path=Path(args.mailbox) if args.mailbox else None,
        plain=args.plain,
        fetch_live=not args.no_live,
        api_base=args.api_base,
    )
    sys.stdout.write(text)
    return 0


def _cli_duty(args: argparse.Namespace) -> int:
    root = Path(args.root) if args.root else repo_root()
    result = scan_duty(
        root=root,
        registry_path=Path(args.registry) if args.registry else None,
        mailbox_path=Path(args.mailbox) if args.mailbox else None,
        stale_hours=args.stale_hours,
        fetch_live=not args.no_live,
        api_base=args.api_base,
    )
    if args.json:
        sys.stdout.write(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    else:
        sys.stdout.write(format_duty_text(result))
    if args.apply:
        if not args.from_name:
            print("--apply requires --from", file=sys.stderr)
            return 2
        apply_nudges(result, from_name=args.from_name, root=root)
    return 0


def main(argv: Optional[Iterable[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="Cursor fleet agent helpers")
    parser.add_argument("--root", default="")
    parser.add_argument("--registry", default="")
    parser.add_argument("--mailbox", default="")
    parser.add_argument(
        "--api-base",
        default=os.environ.get("CURSOR_AGENT_API_BASE", API_BASE_DEFAULT),
    )
    sub = parser.add_subparsers(dest="cmd", required=True)

    pulse = sub.add_parser("pulse", help="print <fleet-pulse>")
    pulse.add_argument("--plain", action="store_true")
    pulse.add_argument("--no-live", action="store_true")
    pulse.set_defaults(func=_cli_pulse)

    duty = sub.add_parser("duty", help="scan fleet duty findings")
    duty.add_argument("--json", action="store_true")
    duty.add_argument("--no-live", action="store_true")
    duty.add_argument("--apply", action="store_true")
    duty.add_argument("--from", dest="from_name", default="")
    duty.add_argument(
        "--stale-hours",
        type=float,
        default=float(os.environ.get("CURSOR_AGENT_DUTY_STALE_HOURS", "4")),
    )
    duty.set_defaults(func=_cli_duty)

    args = parser.parse_args(list(argv) if argv is not None else None)
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
