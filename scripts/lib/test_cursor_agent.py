#!/usr/bin/env python3
"""Unit tests for scripts/lib/cursor_agent.py (no network)."""

from __future__ import annotations

import unittest
from pathlib import Path

from cursor_agent import (
    build_fleet_pulse,
    parse_open_asks,
    parse_registry,
    parse_seats,
    parse_shared_facts,
    scan_duty,
)

FIXTURE_REGISTRY = """
| 显示名 | bcId | 来源 | 当前职责 | 注释 |
|--------|------|------|----------|------|
| **主控2** | `bc-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa` | api | **当前主控**：统筹 | x |
| **主控 造梦** | _(每次 Automations 新开 bcId)_ | automation | auto-dream | skip |
| **分身3** | `bc-bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb` | api | 设计 | y |
"""

FIXTURE_MAILBOX = """
## 共享事实

| 键 | 值 |
|----|-----|
| 产品版本 | **0.1.14** |
| 当前主线任务 | skins |

## 席位状态

### 主控2 · 2026-08-05
- 状态: RUNNING — 编排
- 分支: `main`
- 下一步: duty
- 阻塞: 无

### 分身3 · 2026-08-05
- 状态: IDLE — 等设计合 main
- 分支: —
- 下一步: 待命
- 阻塞: 无

### 请示#1 · 分身3 → 主控 · 2026-08-05
- 问题: demo
- 主控回复: （空着等主控填）
"""


class CursorAgentParseTests(unittest.TestCase):
    def test_registry_skips_placeholder_bcid(self) -> None:
        peers = parse_registry(FIXTURE_REGISTRY)
        names = [p.name for p in peers]
        self.assertEqual(names, ["主控2", "分身3"])
        self.assertTrue(all(p.bc_id.startswith("bc-") for p in peers))

    def test_seats_exact_match_not_substring(self) -> None:
        seats = parse_seats(FIXTURE_MAILBOX)
        self.assertIn("主控2", seats)
        self.assertEqual(seats["主控2"].status, "RUNNING — 编排")
        self.assertNotIn("主控", seats)

    def test_shared_facts(self) -> None:
        facts = parse_shared_facts(FIXTURE_MAILBOX)
        self.assertTrue(any(f.startswith("产品版本=") for f in facts))

    def test_open_asks(self) -> None:
        asks = parse_open_asks(FIXTURE_MAILBOX)
        self.assertEqual(len(asks), 1)
        self.assertIn("请示#1", asks[0])

    def test_pulse_and_duty_offline(self) -> None:
        root = Path(__file__).resolve().parent.parent.parent
        reg = root / "skills/cursor/session-registry.md"
        mail = root / "skills/cursor/mailbox.md"
        pulse = build_fleet_pulse(
            root=root,
            registry_path=reg,
            mailbox_path=mail,
            fetch_live=False,
            plain=True,
        )
        self.assertIn("main:", pulse)
        self.assertIn("peers:", pulse)
        # Placeholder automation row must not appear as a peer line with fake bc.
        self.assertNotIn("_(每次", pulse)

        result = scan_duty(
            root=root,
            registry_path=reg,
            mailbox_path=mail,
            fetch_live=False,
            stale_hours=9999,
        )
        self.assertIn("main", result)
        self.assertIsInstance(result["findings"], list)


if __name__ == "__main__":
    unittest.main()
