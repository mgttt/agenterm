# 有机舰队感知（Fleet Awareness）

目标：用户感觉在和一个**有机体**互动，而不是一堆互不知情的会话。
每个主控/分身在**整个生命周期**都能感知同伴态势，并据此自适应。

## 感知分层

| 层 | SSOT | 新鲜度 | 谁写 |
|----|------|--------|------|
| **身份** | [session-registry.md](session-registry.md) | 低频（换防/开分身） | 主控 |
| **态势** | [mailbox.md](mailbox.md) 共享事实 + 席位块 | 高频（每轮） | 各席位自己 + 主控裁决 |
| **脉搏** | 聊天信封内 `<fleet-pulse>`（由脚本注入） | 投递瞬间 | `cursor_agent_chat.sh` |
| **观测** | MCP `list-cloud-agents` / `batch-fetch-details` | 实时但只读 | 主控优先；分身可只读自检 |

原则：**聊天唤醒，邮箱留证，脉搏同步。** 任一通道失败，其它通道仍能续命。

## 生命周期（每个席位必须遵守）

```text
spawn / wake
  → pull main
  → 读 registry + mailbox 全文（获得同伴态势）
  → 更新本席位心跳（状态/分支/tip/下一步/阻塞）
  → 若有指向自己的指令或请示回复 → 先执行
  → 干活（独占文件集）
  → 里程碑：小步 commit；刷新本席位；必要时 chat 主控/依赖方
  → IDLE/DONE：写清「无新指令不开工」+ deferred
```

分身**不得**假设同伴仍停留在 spawn 时的世界。每一轮 wake 都重新读态势。

## 脉搏块（自动注入）

`scripts/cursor_agent_chat.sh` 默认附带 `--fleet-context`（可用 `--no-fleet-context` 关闭）：
在用户正文前插入压缩的 `<fleet-pulse>…</fleet-pulse>`，含：

- `main` tip（若可得）
- 登记表中的活跃席位 + mailbox 席位一行摘要
- 直播 API 状态（RUNNING/IDLE/ARCHIVED），失败则降级为 registry-only

收件人应把 pulse 当作**当前舰队快照**，再读 mailbox 取细节。

## 自适应协作规则

1. **依赖方未就绪**：写 mailbox 阻塞 + IDLE；勿空转；主控或上游完成后会再 wake。
2. **上游合 main**：下游下一轮 `git pull` 即感知；主控可追加短 chat「可开 Phase N」。
3. **409 busy**：脚本默认等待；授权已写在 mailbox 时，忙方结束后 pull 即可，不依赖第二条 chat。
4. **冲突所有权**：停手 → 请示主控；禁止抢编热点文件。
5. **用户只对主控说话**：主控翻译成有所有权边界的脉冲；分身不直接向用户要全局决策（除非用户点名）。

## Spawn / 换防必须植入的提示词骨架

开分身或换防时，prompt 至少包含：

```text
你是舰队一员。全程遵守 skills/cursor/fleet-awareness.md 与 inter-agent-comms.md。
每轮：pull main → 读 session-registry + mailbox → 更新本席位 → 再干活。
你的显示名：<分身N|主控N>；bcId 以 registry 为准。
文件所有权：…（独占）
禁止：…；合 main：仅主控 / 或你被明确授权。
回报格式：状态/分支/tip/文件清单/证据/阻塞/下一步。
IDLE 也要写「无新指令不开工」。
```

完整换防见 [hand-off-controller.md](hand-off-controller.md)。

## 工具

| 命令 | 用途 |
|------|------|
| `scripts/cursor_agent_chat.sh --from … --to …` | 唤醒；默认注入 fleet-pulse；默认等 409 |
| `scripts/cursor_agent_chat.sh --list` | 直播名册 |
| `scripts/cursor_agent_fleet_pulse.sh` | 打印/刷新本地可见的舰队脉搏（不发 API） |
| MCP `list-cloud-agents` | 主控巡检 |

## 用户体验验收（有机感）

- 用户只跟主控对话，却能感到分身「知道彼此在干什么」。
- 分身回报无需用户转发全文；主控合流后同伴自动对齐。
- 一条短授权 + mailbox 更新，比长篇复述更可靠。
- 失败时降级（无 API 仍可读 git 态势），不假装失忆。
