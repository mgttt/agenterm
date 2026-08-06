# LLM 桥接：免费 Web 会话 + 自配 API（Web-to-API）

| 字段 | 值 |
|------|-----|
| **文档** | 受管 LLM 网关的 Web 会话适配层 + 用户 BYOK 产品设计 |
| **日期** | 2026-08-06 |
| **状态** | 设计稿 rev1 |
| **SSOT 关联** | `prd/PRD_02_13_llm_gateway.md`、`plan/design-cc-hyper-control-agent.md`、`prd/PRD_02_19_inspiration_and_future_vision.md` §INF |
| **非目标** | 在 `agenterm-cc` / `agenterm-rhai` 内嵌 Playwright；Script 权限策略；把 LLM 文本当作 Fleet 操作成功证明 |

---

## 1. 执行摘要

AgenTerm 应提供**统一、本地、OpenAI 兼容**的 LLM 入口（`http://127.0.0.1:<port>/v1/...`），供 Composer、Script、MCP、超控智能体 Intent bar 与未来 Agent harness 消费。

**两类供给并排：**

| 类型 | 用户感知 | 实现本质 |
|------|----------|----------|
| **免费 / 内置 Web 提供方** | 「已登录 DeepSeek / ChatGPT Web，免 API Key」 | 隔离浏览器配置文件 + Playwright/Camoufox 复用**已登录会话**，桥接为 OpenAI 兼容 HTTP |
| **自配 API（BYOK）** | 用户填 OpenAI / Anthropic / DeepSeek API / 本地 Ollama 等 | 直连或经同一网关转发；密钥进 OS 凭据库，**不进** workspace / git / 聊天 |

参考生态（**行为观察，非代码拷贝**）：

| 项目 | 要点 | AgenTerm 可借鉴 |
|------|------|-----------------|
| **WebAI2API** | Camoufox/Playwright；多窗口并发；账号隔离；DeepSeek 等适配 | 每提供方独立 browser profile + 并发槽位模型 |
| **web-model-bridge / web-to-api / LLMs2API / browser-ai-bridge** | 复用已登录浏览器；暴露 OpenAI 兼容接口 | 「Attach 已有会话」vs「托管隔离会话」双模式 |
| **copy.sh / v86**（远期） | 浏览器内 WASM 重计算 | 与 LLM 桥接独立；Intent bar 内嵌预览可选 |

**产品承诺边界：** 免费 Web 提供方依赖目标站点 ToS 与用户自行登录；AgenTerm **不**托管账号密码，**不**绕过 CAPTCHA/风控；会话失效时诚实报错 + 引导重新登录。

---

## 2. 架构：一层网关，多类 Provider

```text
Consumers (同一契约)
  Composer · agenterm-cli · Script (上层策略) · MCP · CC Intent bar
                              │
                              ▼
              ┌───────────────────────────────┐
              │  agenterm-llm-gateway         │  loopback + token auth
              │  OpenAI-compatible /v1/*      │  路由 · 配额 · 审计 · 熔断
              └───────────────┬───────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
 DirectApiProvider    WebSessionProvider      LocalRuntimeProvider
 (BYOK REST)          (Playwright/Camoufox)   (Ollama / llama.cpp)
        │                     │
        │                     ├── ProfileStore (per-provider isolation)
        │                     ├── SessionPool (multi-window / concurrency)
        │                     └── SiteAdapter (DeepSeek / ChatGPT / …)
        │
        └── CredentialStore (OS keychain)
```

### 2.1 与 PRD M9 的关系

`prd/PRD_02_13_llm_gateway.md` 定义**受管网关假设门**（隔离 sidecar、凭据、审计、配额）。本设计是 M9 的 **Provider 插件面**，不是第二个网关：

- **网关** = 唯一 loopback 面 + 策略
- **WebSessionProvider** = 一种「用浏览器当 upstream」的适配器
- 实现 M9 门之前，可先在 `research/` 或独立 sidecar 做**技术探针**；产品化必须过同一网关，禁止 Script/Rhai 直开浏览器。

### 2.2 进程边界

| 组件 | 进程 | 备注 |
|------|------|------|
| `agenterm-llm-gateway` | 可选 sidecar | 4 MiB 级专用 PE；**不**链入 `agenterm-cc` |
| Browser worker | gateway 子进程或 `agenterm-llm-browser` | Playwright/Camoufox 体积大 → **独立**；崩溃不拖 GUI/server |
| CC / Composer | 仅 HTTP 客户端 | 只连 loopback；不 import Playwright |

---

## 3. WebSessionProvider 设计

### 3.1 两种会话来源

| 模式 | 用户操作 | 技术 |
|------|----------|------|
| **Managed profile（推荐默认）** | CC/设置里点「登录 DeepSeek」→ 弹出**隔离**浏览器窗口 | 专用 user-data-dir；与日常 Chrome 分离；可多提供方多 profile |
| **Attach existing（高级）** | 用户指定已有 Chrome/Edge user-data 路径（只读 attach 或 CDP） | 同 web-model-bridge；**显式风险**说明（扩展可见、会话共享） |

### 3.2 多窗口并发与账号隔离（WebAI2API 对齐）

```text
Provider: deepseek-web
├── profile: user-alice@…     → browser context A → slot 0..N
├── profile: user-work        → browser context B → slot 0..N
└── concurrency cap: N (settings + gateway quota)

Request routing:
  model=deepseek-web/chat → pick healthy slot → SiteAdapter.translate → stream back
```

| 概念 | 含义 |
|------|------|
| **Profile** | 独立 cookies/storage；对应「一个登录账号」 |
| **Slot** | 同一 profile 下可并发的 browser context/窗口（站点允许时） |
| **SiteAdapter** | 每站点：DOM/网络拦截、模型列表映射、流式 SSE 翻译为 OpenAI chunk |
| **Health** | 登录态探测；失效 → `provider_session_expired` + UI 引导 |

### 3.3 SiteAdapter 接口（概念）

```text
trait WebSiteAdapter {
  id: "deepseek-web" | "chatgpt-web" | …
  login_flow(): opens managed browser
  probe_session(): ok | expired | blocked
  list_models(): OpenAI model list shape
  chat_completions(req) -> stream
  account_label(): masked email / "未登录"
}
```

首版适配优先级（产品建议，待用户确认）：

1. **DeepSeek Web** — 国内常用；WebAI2API 已有成熟路径
2. **ChatGPT Web** — 国际用户；attach 模式需求高
3. 其余（Claude.ai、Gemini Web）→ Phase 2+

### 3.4 OpenAI 兼容映射

对外统一：

```http
POST /v1/chat/completions
Authorization: Bearer <loopback-token>
{
  "model": "deepseek-web/deepseek-chat",
  "messages": […],
  "stream": true
}
```

`model` 命名：`{provider_id}/{upstream_model_or_alias}`。网关解析 provider，WebSessionProvider 负责翻译；BYOK 则直连 `https://api.openai.com/...`。

---

## 4. BYOK（自配 API）设计

### 4.1 设置面（Settings / CC Extensions 入口）

| 字段 | 存储 | UI |
|------|------|-----|
| Provider 类型 | settings JSON（无密钥） | `openai` · `openai-compatible` · `anthropic` · `ollama` · `custom` |
| Base URL | settings | 默认官方；可改私有网关 |
| API Key | **OS credential store** | 输入后仅显示 `sk-…xxxx` |
| 默认模型 | settings | 下拉（探测 `/v1/models` 或静态列表） |
| 启用 | settings | 与免费 Web 提供方并列；Intent bar 可选 |

### 4.2 路由优先级（用户可配）

```text
default_route:
  1. user-selected model (Intent bar / Composer)
  2. else settings.default_model
  3. else first healthy free web provider
  4. else unavailable + reason
```

**禁止：** 静默把用户 prompt 发到未启用/未登录的提供方。

---

## 5. 超控智能体（CC `hyper_control`）集成

在 `plan/design-cc-hyper-control-agent.md` Intent bar 上增加 **LLM 控制条**（Phase B，布局可先占位）：

```text
┌─ Intent bar ─────────────────────────────────────────────────────────────┐
│ Model: [ deepseek-web ▾ ]  Session: ● logged in · slot 2/4 free          │
│ > _                                                                      │
│ [Submit intent]  [Provider settings]  [Approval queue (0)]              │
└──────────────────────────────────────────────────────────────────────────┘
```

新增 **Provider 状态卡片**（Resource plane 或 Intent 下方窄条）：

| 状态 | 显示 |
|------|------|
| 未配置 BYOK 且无 Web 登录 | `llm_no_provider_configured` + 链接设置 |
| Web 会话过期 | `provider_session_expired` + **[Re-login]** |
| 网关离线 | `llm_gateway_unreachable` |
| 配额耗尽 | `llm_quota_exceeded` + 剩余/重置时间 |
| 正常 | 延迟 p50、当前 model、audit id（无 prompt 正文） |

**Intent 提交路径（Phase B+）：**

```text
User text → gateway /v1/chat/completions (draft/analysis mode)
         → 结构化「建议动作」JSON（非 Fleet receipt）
         → 若含 install/fleet 变更 → Approval queue → server typed action
```

LLM 输出**永远不能**单独完成 Fleet 变更；与 PRD M9 一致。

---

## 6. 安全与合规

| 项 | 规则 |
|----|------|
| 凭据 | API Key 仅 OS store；Web cookies 仅在 ProfileStore 目录 |
| 日志 | 默认不记录 prompt/response；审计仅 route/latency/tokens/cost/decision |
| 网络 | 网关默认 loopback；出站 allowlist（M9） |
| 子进程 | Browser worker 无 PTY/workspace 继承 |
| ToS | 设置与首次登录显式声明：Web 桥接为用户自行登录会话，AgenTerm 非官方 API |
| Agent 策略 | 批准/配额在 **Agent harness / gateway**，不在 Rhai profile |

---

## 7. 研究目录与体积

| 路径 | 用途 |
|------|------|
| `research/agenterm-llm-bridge/` | Playwright/Camoufox spike；OpenAI shim；**不进** release PE |
| `research/agenterm-webview/` | CC 壳；**不**承担 Playwright（避免 WRY+Playwright 双栈） |

Release 路径：独立 `agenterm-llm-gateway`（+ 可选 browser worker PE），体积预算单独核算，**不占用** `agenterm-cc` 4 MiB。

---

## 8. 分阶段交付

| 阶段 | 交付 | 证据 |
|------|------|------|
| **R0 研究** | `research/agenterm-llm-bridge`：单 SiteAdapter + curl 调 `/v1/chat/completions` | 本地脚本 + 无密钥日志 |
| **R1 网关骨架** | loopback gateway；BYOK 单 provider；`agenterm-cli llm ping` | 单元 + 黑盒 |
| **R2 Web 单站** | Managed profile + DeepSeek Web；登录/探测/流式 | 隔离 profile 目录 smoke |
| **R3 并发** | Profile + slot 池；多请求不互踢 | 并发 Rhai/CLI 测试 |
| **R4 CC 投影** | Intent bar model 选择 + 状态卡片（可 unavailable） | `cc-snapshot` 字段 |
| **R5 M9 门** | 审计/配额/熔断对齐 PRD_02_13 | qualification 条目 |

---

## 9. 开放问题（待主控/用户）

| ID | 问题 | 建议 |
|----|------|------|
| LQ-1 | 免费 Web 提供方首站 | **DeepSeek Web** |
| LQ-2 | Browser 引擎 | Camoufox（反检测）vs  stock Chromium；可配置 |
| LQ-3 | 是否 ship「Attach 已有浏览器」 | Phase 2；默认仅 Managed profile |
| LQ-4 | 网关 loopback 端口 | 固定 `17421` vs 动态 + instance 文件 |
| LQ-5 | Composer 默认 model | 跟随 settings vs 每 workspace 记忆 |
| LQ-6 | 与 Cursor Cloud Agent 关系 | 独立；CC roster 只投影，不经此网关代操 Cursor |
| LQ-7 | 免费层商业表述 | 「内置桥接能力」vs「推荐提供方」；法务/ToS 审阅 |

---

## 10. 文档交叉引用

- CC 布局：`plan/design-cc-hyper-control-agent.md` §4.2 Intent bar
- 网关假设门：`prd/PRD_02_13_llm_gateway.md`
- 可执行体族：`prd/PRD_02_02_executable_family.md`（`agenterm-llm-gateway.exe`）
- 平台层 INF 分支：`prd/PRD_02_19_inspiration_and_future_vision.md`
