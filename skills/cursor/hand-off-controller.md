# 主控换防 / 接管

把统筹权交给新的 Cloud Agent 会话（主控N），旧主控转待命。

## Checklist

1. **开新会话**（API 不能改名，创建时 `name` 就写成 `主控2` / `主控3`）：
   见 [spawn-sibling-cloud-agent.md](spawn-sibling-cloud-agent.md)。
2. **首条 prompt** 必须自洽（新会话无历史），至少含：
   - 你是当前主控；读 `fleet-awareness.md` + `AGENTS.md` + `PRD.md`
   - `git pull --ff-only origin main`；确认 tip
   - 更新 `session-registry.md`：自己=当前主控；旧主控=已换防/待命
   - 活跃分身 bcId/职责（可引用 registry）；**暂不派分身**则写明
   - 互通：`cursor_agent_chat.sh --from 主控N`；邮箱仍是 durable SSOT
3. **小步 commit + push main**（或短命 `cursor/…` 再合）；勿堆无必要 PR。
4. **旧主控**：chat 一条换防补充；标 IDLE/待命；勿再当唯一统筹。
5. **分身**：不必全部重开；下一轮 pull 即见新主控登记。

## 提示词模板（精简）

```text
你是【主控N】。换防自 <旧主控 bcId>。从现在起由你统筹 partnernetsoftware/agenterm。

立即：
1. git fetch && git checkout main && git pull --ff-only origin main
2. 读 AGENTS.md、PRD.md、skills/cursor/{fleet-awareness,session-registry,mailbox,inter-agent-comms}.md
3. 更新 session-registry：你=当前主控；旧主控=已换防/待命
4. 小步 commit+push main
5. 自报 bcId/URL 与 tip SHA；等用户下一指令

约束：遵循 AGENTS.md；舰队感知见 fleet-awareness.md；密钥不进 git/聊天。
旧席位（可观测，勿默认派活）：…
```

## 勿做

- 指望 REST 改显示名。
- 换防后不更新 registry（分身会认错主控）。
- 并行两个「当前主控」同时合流。
