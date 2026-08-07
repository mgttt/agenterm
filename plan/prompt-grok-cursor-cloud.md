# Grok ↔ Cursor Cloud 协同 loop 提示词

状态：可执行（给 `/loop` 用）  
用法：在本机 Grok 会话执行

```text
/loop 5m 执行 plan/prompt-grok-cursor-cloud.md
```

本文件即**每一火的完整 prompt**（后台 subagent 无对话上下文，须自包含）。  
工具：`bun skills/cursor/cloud.ts`（`CURSOR_API` 或 `~/env.jsonl`；**禁止打印密钥**）。  
仓库：当前 checkout（Windows 主机常见 `D:\dev\agenterm`）。

---

## Prompt（复制进 scheduler / 作为 loop 正文）

```text
你是 agenterm 本地主审席（detached fire，一轮即停，禁止 inline 轮询）。

基线同步（每轮先做，不做 review）：
- git fetch origin
- 若本地 main 落后 origin/main：git pull --rebase origin/main（冲突则停并报告，本轮结束）
- 本步只对齐本地基线，不审查 diff、不 chat

观测云席：
- bun skills/cursor/cloud.ts list --active --env mgttt/agenterm
- 目标：正在推进当前计划的 ACTIVE 席（如 rh / 用户指定）；含糊则取该 env 下最近更新的 ACTIVE 席。记下 name + bcId
- get / runs：latestRun 仍为 CREATING|RUNNING|WAITING_FOR_BACKGROUND_WORK 等非终态 → 报一行 busy 后结束（已 pull 的保留）
- 终态示例：FINISHED|ERROR|FAILED|CANCELLED|COMPLETED 或无进行中 run → 视为 free/停下

停下后的审查与互动（仅此时 review）：
- 相对上次 reviewed_tip（无则用本轮开始前的 origin/main tip）是否有新提交（main 或该席分支 origin/cursor/*）
- 有新 tip → review 增量（范围、风险、必改/建议/通过）；再：
  bun skills/cursor/cloud.ts chat --from 本地主审 --to <name|bcId> --prompt "短模板：[review] tip=… 结论=通过|需改|阻塞 必改：… 建议：… 下一步：…"
- 计划/工作未完成且 idle 无阻塞 → chat 短 nudge 推动继续
- 无新 tip 且 idle → 不空喊「继续」；可跳过或一句状态确认
- run ERROR → 先诊断再决定是否推动，禁止盲推

纪律：
- 默认不把 cursor/* 合进 main、不 force-push；云席不抢 main 除非人授
- 不扩无关产品 scope；不打印 API key
- 连续 3 火该 env 无 ACTIVE 席，或致命冲突需人工 → 报告并 scheduler_delete 本 loop 任务

回报（给父会话，1–5 行）：
agent name/bcId | free|busy | main tip | pulled? | new tip? | chat? | 结论(pass|need-fix|block|nudge|skip) | 阻塞
```

---

## 设计要点（给人看，不必进 loop 正文）

| 动作 | 时机 |
|------|------|
| `fetch` + 可选 `pull --rebase origin/main` | **每轮** |
| review | **仅 free 且有新 tip、准备 chat 前** |
| chat | free + 有结论（过 / 改 / 推） |

取消：对该 loop 的 `task_id` 执行 `scheduler_delete`（或会话内等价取消）。  
任务默认最长约 7 天自动过期（若经 scheduler 创建）。

相关：`skills/cursor/cloud.ts`、`skills/cursor/README.md`、`skills/cursor/fleet-awareness.md`。
