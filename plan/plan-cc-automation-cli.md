# Control Center 自动化 / CLI 测试接口方案

> 状态：设计稿（2026-08-06）  
> 触发：全自动开发需要截图、鼠标、键盘；同时担心 CC「合并到主程序」后不好做 CLI。  
> 关联：`prd/PRD_02_21_control_center.md`，现有 `agenterm-cc screenshot|snapshot|…`，终端侧 `ui-action` / lease 中继。

---

## 1. 问题拆开

| 顾虑 | 实质 |
|------|------|
| CC 合并进主程序后难做 CLI | **入口形态**（`agenterm-cc` vs `agenterm cc`）≠ **自动化通道** |
| 要截图 / 键鼠 | 需要 **可寻址的 UI 表面** + **有界、可等待、可证据化** 的命令 |
| 别变成第二个 Fleet 权威 | 所有「真状态」仍来自 `agenterm server`；CC 只投影与本窗交互 |

结论：**CLI 永远对着「CC 进程的控制面」说话，而不是对着主 GUI 的 PTY 面说话。**  
主程序是否共用一个 PE，只改变 *怎么 spawn*，不改变 *命令语义*。

---

## 2. 推荐架构（三层）

```text
                    ┌─────────────────────────────┐
  人类 / 测试 / 脚本 │  agenterm-cli  (或 cc-cli)   │  公共入口、JSON、wait
                    └─────────────┬───────────────┘
                                  │ typed control plane
                    ┌─────────────▼───────────────┐
                    │  agenterm server            │  Fleet 真相（tab/PTY/…）
                    │  + 可选：转发 CC 命令到 lease │  （仅当动作是「对某 server 做 inspect」）
                    └─────────────┬───────────────┘
                                  │
           ┌──────────────────────┼──────────────────────┐
           │                      │                      │
           ▼                      ▼                      ▼
   主终端 GUI                CC 进程                 未来 WebView 壳
   (ui-action 中继)          (cc-action 中继)         (同一契约)
```

### 2.1 稳定入口（对外）

**始终保留一个「控制面客户端」名字**，不要让人猜是主程序还是子 PE：

```text
# 现状（保留）
agenterm-cli control-center open|status|snapshot|close|…
agenterm-cc  open|status|snapshot|screenshot|capabilities|…

# 合并后的推荐（二选一或并存）
agenterm-cli cc <subcmd> …          # 首选：所有自动化从 cli 进
agenterm cc <subcmd> …              # 若 PE 合并：主程序子命令只负责「起壳」
agenterm-cc …                       # 过渡别名 → 转发到同一实现
```

原则：

1. **`agenterm-cli cc *` 是自动化 SSOT**（脚本、smoke、agent 只用它）。  
2. **`agenterm cc` / `agenterm-cc` 只负责进程生命周期**（open/focus/no-activate），或薄封装同一库。  
3. 合并 PE 时：**删除的是「第二个发布物」**，不是「第二套命令语义」。

### 2.2 CC 进程内：与主 GUI 同构的「命令中继」

主终端已经验证过的模式（**不要发明第二套**）：

| 主 GUI | CC 对应 |
|--------|---------|
| replaceable UI lease | **CC interactive lease**（每用户域默认一个） |
| 服务器队列 `ui-action` | **`cc-action`**（或 `ui-action` 带 `surface=cc`） |
| `ui-snapshot` | **`cc-snapshot`**（已有雏形 `agenterm-cc snapshot`） |
| GUI poll → apply → complete | 同一生命周期，超时/有界/可过期 |

伪流程：

```text
cli:  cc-action click --target toolbar.workflows
  → server 或本地 registry：找到 live CC owner (pid + start_identity)
  → 入队 command_id
  → CC 消息环 poll
  → 执行 / 截图 / 点按
  → complete(response_json)
  → cli wait 同一 command_id
```

**关键：命令完成必须在「当前 owner」上 ack**（主 GUI 的 S1 rebind 教训：切身份后再 complete 会 `ui_client_command_unknown`）。

### 2.3 真源 vs 窗面

| 数据 | 权威 | 命令示例 |
|------|------|----------|
| tab 列表、PTY 状态、epoch | `agenterm server` | 现有 `list-windows` / `ui-snapshot`（headless） |
| CC 导航选中、侧栏展开、本地 draft | **CC 进程** | `cc-snapshot` / `cc-action select-nav` |
| PNG 像素 | **CC 窗口 owner** | `cc screenshot`（已有） |
| 键鼠 | **CC 窗口 owner** | `cc-action key|click|move|scroll` |

禁止：让 CLI 直接 `SendInput` 到「随便一个 HWND」而不校验 registry owner。

---

## 3. 命令面设计（面向全自动开发）

### 3.1 生命周期（已有，补齐别名即可）

```text
agenterm-cli cc open [--no-activate] [--instance NAME]
agenterm-cli cc status|close [--json]
agenterm-cli cc capabilities [--json]
```

### 3.2 观察（必须 100% 覆盖鼠标可见状态）

```text
agenterm-cli cc snapshot [--json]
  → 返回：窗口几何、可见性、focus surface、导航选中、可点 hit-target 列表、
          server context、renderer、last_error、event 游标（若有）

agenterm-cli cc wait
  --nav NAME | --focus SURFACE | --modal KIND|none
  --title-contains S | --server-state connected|offline|…
  --timeout-ms N

agenterm-cli cc screenshot --output PATH [--json]
  → 已有；要求：不抢焦点；owner pid/start_identity 校验；digest 回执
```

`snapshot` 必须带 **稳定 target id**（见下），不能只靠像素坐标。

### 3.3 交互（键鼠）

```text
agenterm-cli cc-action click  --target <id> [--button left|right] [--clicks 1]
agenterm-cli cc-action move   --target <id> | --x N --y N   # 坐标相对 CC 客户区
agenterm-cli cc-action scroll --target <id> --delta N
agenterm-cli cc-action key    --code <vk|name> [--down|--up|--chord]
agenterm-cli cc-action type   --text "…"                     # 有界长度
agenterm-cli cc-action focus  --target <id>
agenterm-cli cc-action select-nav --name cockpit|workflows|… # 语义动作优先于坐标
```

规则：

1. **语义动作优先**（`select-nav`、`open-inspect`）→ 坐标动作兜底。  
2. 每个 hit-target 在 `cc-snapshot` 里有：`id`、`role`、`bounds`、`enabled`、`visible`。  
3. 坐标动作失败时返回 typed error：`target_not_found` / `target_disabled` / `out_of_bounds` / `owner_mismatch`，**禁止假成功**。  
4. 与主 GUI 一样：高风险动作（关闭 server、装包）必须显式子命令，不进通用 `click`。

### 3.4 与「主程序 ui-action」的边界

| 前缀 | 表面 | 例子 |
|------|------|------|
| `ui-action` | 主终端 HWND | tabs、composer、server-strip、settings |
| `cc-action` | Control Center HWND | 导航、列表、诊断、未来 WebView chrome |
| 无前缀 Fleet | server 真相 | `list-windows`、`send-keys`（PTY） |

自动化脚本应写：

```text
# 对 Fleet 真相
agenterm-cli --instance main list-windows
# 对主窗 UI
agenterm-cli --instance main ui-action select-server-tab --name work
# 对 CC 窗 UI
agenterm-cli cc open --no-activate
agenterm-cli cc-action select-nav --name cockpit
agenterm-cli cc screenshot --output %TEMP%\cc.png
```

---

## 4. 「合并到主程序」三种形态对照

| 形态 | spawn | CLI | 推荐 |
|------|-------|-----|------|
| **A. 独立 PE `agenterm-cc`（现状）** | `agenterm-cc` | `agenterm-cli control-center` + `agenterm-cc screenshot` | 过渡期 OK |
| **B. 同 PE 子命令 `agenterm cc`**（对齐 `agenterm server`） | `agenterm cc` 另进程 | **仍用 `agenterm-cli cc *`** | **推荐合并终点** |
| **C. 嵌进主 GUI 进程** | 无独立进程 | 只能 `ui-action` 扩 surface | **不推荐**（崩溃/锁文件/lease 缠死） |

**明确建议：合并也要保持「独立 OS 进程」**（同 `server` 模式：同 PE、不同进程）。  
这样：

- 锁文件问题可控（停 CC 不必停 server）；  
- 自动化仍按 registry owner 定位；  
- WebView 崩溃不拖死主终端。

---

## 5. 实现分期（可落地）

### P0 — 契约不扩表面（1 小步）

1. 统一文档：`agenterm-cli control-center` ≡ 未来 `cc` 别名表。  
2. `snapshot --json` 补齐 **hit-targets[]**（哪怕只有现有导航/按钮）。  
3. `screenshot` 已有 → smoke 固定走 CLI，禁止「人眼看一眼」。

### P1 — 命令中继（主路径）

1. CC 注册 interactive lease（或复用轻量 command queue 文件/管道，**优先复用 server 上已有 ui-command 队列模式**）。  
2. 落地 `cc-action`：`select-nav` / `focus` / `click --target` / `key`。  
3. `wait` 与主 GUI `wait-ui` 对称。

### P2 — 全自动开发友好

1. 坐标 + 语义混用；PNG + snapshot 双证据。  
2. Rhai smoke：`cc-smoke.rhai` 只调用 public CLI。  
3. WebView 渲染器切换时：**同一 target id**，renderer 只换像素路径。

### 明确非目标

- 在 CC 里跑无界 `SendInput` 全局键鼠。  
- 让 WebView JS 成为第二权威。  
- 把 PTY `send-keys` 伪装成 CC 自动化。

---

## 6. 与当前代码的衔接点

| 已有 | 用途 |
|------|------|
| `agenterm-cc snapshot/screenshot/capabilities` | P0 观察面 |
| `agenterm-cli control-center open|…` | 生命周期 |
| 主 GUI `ui-action` + lease complete | **抄中继，不要重造** |
| `research/agenterm-webview` bridge-v1 | 未来 WebView 的消息边界参考（仍无 product bridge） |
| `agenterm server` 子命令 | Fleet 真相；CC 永远只是 client |

---

## 7. 验收标准（以后实现时）

1. **无鼠标**：仅 CLI 能 open → wait 某 nav → screenshot → 校验 PNG 非空 + snapshot 字段。  
2. **有 target id**：改布局后旧坐标脚本可挂，但 `--target` 脚本仍绿。  
3. **owner 校验**：杀 CC 进程后 `cc-action` / `screenshot` typed fail，不假成功。  
4. **权威不串**：CC 崩溃不杀 `agenterm server`、不丢 PTY。  
5. **合并后**：`agenterm cc` 与 `agenterm-cli cc` 行为一致（或后者为唯一文档入口）。

---

## 8. 建议拍板项（需要你点头后再开工实现）

1. **合并形态选 B**（`agenterm cc` 同 PE 另进程），CLI 入口固定为 **`agenterm-cli cc`**？  
2. 命令名 **`cc-action` / `cc-snapshot`** 还是扩写现有 `control-center …` 子命令？  
3. P1 是否必须先做 **语义 nav**，坐标 click 放 P2？

你确认后，可按 P0→P1 开实现叶子；本文件作 SSOT 交接。
