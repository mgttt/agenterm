# 主控 ↔ 分身互通协议

API 单向推送（`POST /v1/agents/{id}/runs`）在分身 `RUNNING` 时常
`409 agent_busy`，且分身之间没有共享聊天。因此**仓库邮箱是互通 SSOT**，
API 只做紧急打断。

## 角色

| 角色 | 责任 |
|------|------|
| **主控** | 写任务简报、裁决冲突、合流 `main`、更新登记表；轮询邮箱与 MCP |
| **分身** | 开工前读邮箱；里程碑后写本席位；需要裁决时写「请示」；不静默结束 |

## 文件

| 路径 | 用途 |
|------|------|
| [session-registry.md](session-registry.md) | 身份、bcId、当前职责（低频） |
| [mailbox.md](mailbox.md) | 共享事实、席位状态、请示队列、交接日志（高频） |
| 本文件 | 协议本身 |

勿把密钥、token、完整 `CURSOR_*` 值写入邮箱。

## 开工清单（每个分身每轮）

1. `git fetch && git pull --ff-only origin main`（或当前工作分支）。
2. 读 `session-registry.md` + `mailbox.md` 全文。
3. 若邮箱有指向自己的 **主控指令** 或未决 **请示回复**，先执行再开新话题。
4. 更新本席位「状态」块：`状态 / 分支 / main基准 / 下一步 / 阻塞`。
5. 小步 `commit` + `push` 邮箱改动（可与代码同提交，或单独
   `docs(skills): mailbox …`）。
6. 需要主控立刻看见时，用 `scripts/cursor_agent_chat.sh --from … --to …`；默认会等忙结束再投递。短超时可用 `--wait-timeout 60`；明确不想等则 `--no-wait`（此时可回落邮箱）。

## 主控清单

1. 派活前写清：目标、禁止项、文件所有权、成功证据、是否允许合 `main`。
2. API 推送优先走 `cursor_agent_chat.sh`（自适应解析显示名、默认等待 409）；观测用 MCP `list-cloud-agents` / `batch-fetch-details`。
3. 大 transcript 用子代理；合流后更新邮箱「共享事实」里的 `main` SHA 与版本号。
4. 邮箱仍是 durable 状态/请示 SSOT；聊天不能代替席位状态块。

## 席位状态模板

每个席位只保留**最新一块**（覆盖旧块，勿无限堆积）：

```markdown
### 分身N · YYYY-MM-DD HH:MM UTC
- 状态: RUNNING|IDLE|BLOCKED|DONE
- 分支: cursor/…
- main基准: <sha7>
- 本轮目标: …
- 已完成: …
- 证据: CI链接 / 测试命令 / artifact路径
- 阻塞/请示: 无 | 见下方请示#K
- 下一步: …
```

## 请示模板（需要主控裁决）

追加到 `mailbox.md`「请示队列」，编号递增：

```markdown
### 请示#K · 分身N → 主控 · YYYY-MM-DD HH:MM UTC
- 问题: …
- 选项: A) … B) …
- 建议: …
- 影响文件: …
- 主控回复: （空着等主控填）
```

主控回复后把 `主控回复` 填满，并把该请示标为 `已决`；分身下一轮必须先读回复。

## 交接日志

`mailbox.md`「交接日志」只追加最近条目（建议保留 ≤20 行），一行一事：

```text
YYYY-MM-DD HH:MM UTC | 谁 | 做了什么 | 指向（sha/PR/run）
```

## 与 API / MCP 的关系

| 通道 | 用途 | 限制 |
|------|------|------|
| 邮箱（git） | 状态、请示、事实、所有权 | 需 pull/push；有短暂冲突时 rebase 邮箱提交 |
| REST 推送 (`scripts/cursor_agent_chat.sh`) | 主通道唤醒/改向；信封 `<from::…><to::…>` | 默认 `--wait`：409 时探活+退避重试；`--no-wait` 立即失败；无回执线程 |
| MCP `list/batch-fetch` | 主控观测 | 只读；不能代替分身写状态 |
| MCP `mem_*` | 租户偏好/指针（可选） | **不是**工程 SSOT；细则以本仓库为准 |

## 禁止

- 只靠聊天记忆传递「当前 main / 职责 / 是否可合」。
- 分身结束任务却不更新邮箱。
- 两个分身同时编辑同一热点文件（`Cargo.toml`、`ci.yml`、`agenterm.tasks.json` 等）而不经主控分配所有权。
- 把 QEMU/Wine 限制误写成「本云环境不能跑原生 Linux GUI」。
