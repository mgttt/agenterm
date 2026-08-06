# LLM 网关 — Rhai 逻辑包架构（Native Shell + Logic Pack）

| 字段 | 值 |
|------|-----|
| **文档** | `agenterm-llm-gateway` 可执行体与可热更新逻辑包 split |
| **日期** | 2026-08-06 |
| **状态** | 设计稿 rev1 |
| **上级** | `plan/design-llm-bridge-web-to-api.md`、`prd/PRD_02_13_llm_gateway.md` |
| **非目标** | 用 Rhai profile 当授权边界；把 Playwright 塞进 `agenterm-rhai.exe` 通用路径 |

---

## 1. 问题与结论

**问题：** Web 提供方 DOM/API、路由策略、模型别名、降级链**变更频繁**；若全部编译进 `agenterm-llm-gateway.exe`，每次站点改版都要发新版 PE。

**结论：** 自建大模型网关采用 **两层 split**：

| 层 | 产物 | 变更频率 | 交付 |
|----|------|----------|------|
| **Native Shell** | `agenterm-llm-gateway.exe` (+ 可选 `agenterm-llm-browser.exe`) | 低（HTTP、凭据、审计、浏览器 IPC） | 随 AgenTerm 版本 / softmgr |
| **Logic Pack** | Rhai 模块 + `gateway.manifest.json` | **高**（SiteAdapter、路由、映射） | **独立包**；reload 无需重启 PE |

Logic Pack 复用仓库已成熟的 **Script 任务/manifest 范式**（`agenterm.tasks.json`、`script_api` 版本、`capabilities` 声明），但命名空间与开发任务**分离**，避免把 release 脚本和 LLM 路由混在一个 manifest。

---

## 2. 总体架构

```text
Consumers → loopback OpenAI /v1/* 
                │
                ▼
┌─────────────────────────────────────────────────────────────┐
│ agenterm-llm-gateway.exe  (Native Shell)                     │
│  · HTTP server · loopback token · quotas/circuit (counters)  │
│  · audit sink · credential bridge (OS store)                 │
│  · pack loader / verify / hot-reload                         │
│  · Rhai engine (gateway-scoped host API only)                │
│  · browser worker IPC                                        │
└───────────────────────────┬─────────────────────────────────┘
                            │ loads
                            ▼
┌─────────────────────────────────────────────────────────────┐
│ Logic Pack  (versioned directory or .agp)                    │
│  gateway.manifest.json                                       │
│  router.rhai          ← route(model, req) → provider call      │
│  providers/deepseek-web.rhai                                   │
│  providers/openai-byok.rhai                                    │
│  lib/transform.rhai   ← OpenAI chunk ↔ upstream               │
└───────────────────────────┬─────────────────────────────────┘
                            │ native calls
                            ▼
              Browser worker · HTTP client · credential store
```

**AGENTS.md 对齐：**

- **授权/配额/出站策略**在 Native Shell + 未来 Agent harness；**不在** Rhai pack 里藏 allowlist。
- Rhai pack 的 `capabilities` 仅表示 **host API 发现与兼容**（Script 语义），不是许可/grant。
- `agenterm-rhai.exe` 仍为通用 unrestricted runtime；网关 pack **不**要求用户改全局 Rhai profile。

---

## 3. Native Shell 职责（少改、厚信任）

| 模块 | 职责 | 禁止下沉到 pack |
|------|------|-----------------|
| **HTTP 面** | `127.0.0.1` 监听、`/v1/models` `/v1/chat/completions` | — |
| **Loopback auth** | Bearer token / instance 文件 | pack 不能关 auth |
| **Pack 治理** | 哈希/签名验证、schema 版本、加载/卸载、reload | — |
| **凭据** | OS keychain 读写；pack 仅收 **handle id** | 密钥明文进 Rhai |
| **审计** | route、latency、tokens、decision；**默认无 prompt 正文** | — |
| **配额/熔断** | 计数器、deadline、max_output；pack 可 **建议** route，native **裁决** deny | pack 绕过 quota |
| **Browser IPC** | 起停 worker、profile 路径、CDP 帧 | Playwright 链进 Rhai 通用 runtime |
| **出站 HTTP** | 经 native 客户端；目的地由 **settings + M9 allowlist** 决定 | pack 内任意 URL |

PE 体积：独立预算；Browser worker **另 PE**，避免 gateway 本体膨胀。

---

## 4. Logic Pack 职责（常改、薄逻辑）

适合放 Rhai 的逻辑（站点改版只发 pack）：

| 内容 | 示例 |
|------|------|
| **Provider 注册** | id、display、model 前缀、health 探针脚本名 |
| **路由** | `deepseek-web/chat` → provider + slot 选择 + fallback 链 |
| **请求翻译** | OpenAI messages → 站点 JSON / form / DOM 步骤序列 |
| **响应翻译** | SSE / WebSocket / DOM scrape → OpenAI stream chunks |
| **模型列表** | 静态表 + 动态 probe 合并 |
| **错误映射** | 站点错误 → `provider_session_expired` 等稳定 reason |
| **特性开关** | manifest 内 `features`（仍由 native 执行硬限制） |

**不适合**放 Rhai（保留 native 或 browser worker 内 Rust/Node）：

- TLS、HTTP/2 栈、连接池（除非经 `std.net` 且受 allowlist）
- 大规模 HTML 解析性能路径（可 WASM 模块 + native 加载，远期）
- 反爬指纹（Camoufox 配置在 worker；pack 只调 `browser.new_context(opts)`）

---

## 5. Pack 格式（对齐 Script manifest 范式）

### 5.1 目录布局

```text
llm-gateway-pack/
├── gateway.manifest.json      # SSOT：版本、requires、providers、entry
├── router.rhai                  # fn route(ctx, req) -> RouteDecision
├── providers/
│   ├── deepseek-web.rhai
│   ├── chatgpt-web.rhai
│   └── openai-direct.rhai
└── lib/
    ├── openai_types.rhai
    └── stream_map.rhai
```

### 5.2 `gateway.manifest.json`（草案）

```json
{
  "schema_version": 1,
  "pack_id": "agenterm-llm-pack-builtin",
  "pack_version": "2026.08.06.1",
  "requires": {
    "gateway_shell": { "minimum": "0.1.0", "maximum": "0.99.99" },
    "script_api": { "minimum": 2, "maximum": 2 },
    "capabilities": [
      "llm.route",
      "llm.provider.register",
      "llm.http.forward",
      "llm.browser.session",
      "llm.credential.handle",
      "llm.audit.emit",
      "std.json.parse",
      "std.time.system-time-now"
    ]
  },
  "entry": "router.rhai",
  "providers": [
    { "id": "deepseek-web", "module": "providers/deepseek-web.rhai", "kind": "web_session" },
    { "id": "openai-direct", "module": "providers/openai-direct.rhai", "kind": "byok" }
  ],
  "provenance": {
    "sha256": "…",
    "channel": "builtin"
  }
}
```

与 `agenterm.tasks.json` **同构思想**（`requires.script_api` + `capabilities`），但：

- 独立 schema：`gateway.manifest.json`，不占用开发 task manifest；
- `gateway_shell` 范围约束 pack 与 PE ABI；
- pack 由 gateway loader 加载，**不是** `agenterm-rhai task run`。

### 5.3 Gateway-scoped Host API（`llm.*`）

Pack 内可调用的 native 注册函数（示意）：

| API | 用途 |
|-----|------|
| `llm.route_register(prefix, provider_id)` | 注册 model 前缀 |
| `llm.http_forward(handle, method, path, body, headers)` | BYOK；native 注入 auth |
| `llm.browser_session(provider_id, profile_id)` | 取 slot handle |
| `llm.browser_call(handle, op, args)` | navigate / eval / intercept |
| `llm.credential_get(handle_id)` | **不**返回 secret；仅「是否存在」 |
| `llm.audit(event_json)` | 结构化审计事件 |
| `llm.emit_chunk(stream_id, openai_chunk)` | 流式回写 |
| `llm.deny(reason_code, message)` |  typed 失败 |

开发期可用 `agenterm-rhai` + mock host 测 pack；生产由 gateway PE 内嵌同名 API。

---

## 6. 更新与热加载

### 6.1 Pack 来源（channel）

| Channel | 路径 | 更新方式 |
|---------|------|----------|
| **builtin** | 随安装包 / repo `packs/llm-gateway-builtin/` | AgenTerm 升级 |
| **user** | `~/.local/share/agenterm/llm-gateway/packs/<id>/` | 用户/脚本覆盖 |
| **softmgr**（远期） | 签名 `.agp` | 自动更新 + rollback |

默认：**builtin + user overlay**（user 同 id 覆盖 builtin，显式版本号）。

### 6.2 Reload 语义

```text
agenterm-cli llm-gateway reload [--pack <id>] [--wait]
  → native: drain in-flight (deadline) → unload Engine → load manifest → compile entry
  → 成功: pack_version 变更；audit pack_reload
  → 失败: 保留旧 pack；reason pack_load_failed / script_api_incompatible
```

| 策略 | 行为 |
|------|------|
| **In-flight 请求** | 完成或 cancel；reload 有 bounded wait |
| **Browser worker** | **尽量不重启**；仅 pack 逻辑变；profile 不变 |
| **Watch**（可选） | user channel 目录 mtime → debounce reload |
| **Rollback** | 保留 `packs/.previous/` 一代；`llm-gateway rollback` |

**产品承诺：** 站点适配更新 = **发新 logic pack**（或 user 目录 drop-in），**不必**发新 `agenterm-llm-gateway.exe`，除非 `gateway_shell` ABI 或 host API 变更。

### 6.3 版本与兼容

```text
gateway PE 0.1.x  +  script_api 2  +  pack pack_version 2026.08.06.1
                      ↑                      ↑
              agenterm-rhai 大版本对齐    站点 DOM 改版只 bump 这个
```

- `script_api` 不兼容 → 必须升 PE（或升 gateway_shell）
- 仅 provider Rhai 改 → 只升 pack

---

## 7. 请求路径（端到端）

```text
POST /v1/chat/completions
  → Native: auth · quota · parse JSON
  → Rhai router.route(ctx, req)
       → pick provider module
       → providers/deepseek-web.rhai::chat(ctx, req)
            → llm.browser_session(...)
            → llm.browser_call(navigate / intercept / ...)
            → llm.emit_chunk(...)  × N
  → Native: audit complete · quota decrement
```

BYOK 路径：`openai-direct.rhai` 调 `llm.http_forward`，native 从 keychain 取 key。

---

## 8. 与 Web-to-API 设计的关系

| 原设计概念 | 新归属 |
|------------|--------|
| SiteAdapter trait | **`providers/*.rhai`** |
| 路由优先级 | **`router.rhai` + settings JSON** |
| Profile / Slot 池 | **Native browser worker**；pack 只选 profile_id |
| OpenAI 映射 | **`lib/openai_types.rhai` + provider** |
| CC Model 下拉 | 读 native **`/v1/models`**（聚合 pack 注册） |

上级文档 `design-llm-bridge-web-to-api.md` 的 R0–R5 阶段仍成立；增加：

| 阶段 | 增量 |
|------|------|
| **R1.5** | 空 pack + `reload`；BYOK 全在 Rhai |
| **R2** | `deepseek-web.rhai` + browser host API |
| **R2.5** | user channel drop-in；watch reload |
| **R5** | softmgr 签名 pack |

---

## 9. 仓库落点（规划）

| 路径 | 用途 |
|------|------|
| `packs/llm-gateway-builtin/` | 内置 logic pack（随 repo） |
| `research/agenterm-llm-bridge/` | pack 开发探针 + mock host |
| `docs/llm-gateway-pack.md` | pack 作者指南（实现时写） |
| `src/bin/agenterm-llm-gateway/` | Native Shell（未来） |

**禁止：** 把 pack 塞进 `scripts/rhai/` 开发 task 树而不分 manifest；禁止 pack 调用 unrestricted `agenterm-rhai task run` 间接起浏览器。

---

## 10. 开放问题

| ID | 问题 | 建议 |
|----|------|------|
| GP-1 | Pack 内嵌引擎 vs 子进程 `agenterm-rhai` | **内嵌** slim host（低延迟）；开发用 CLI mock |
| GP-2 | user pack 是否默认允许 | **允许** user channel；unsigned 需 settings 显式启用 |
| GP-3 | pack 自动更新源 | 远期 softmgr；近程 git/手动 |
| GP-4 | SiteAdapter 是否允许 WASM 辅助解析 | Phase 2；native 加载 wasm 模块，Rhai 调 wasm export |
| GP-5 | 与 `agenterm.tasks.json` 共用 script_api 2 | **是**；host API 列表是 gateway 子集 |

---

## 11. 交叉引用

- Web 桥 + BYOK 产品面：`plan/design-llm-bridge-web-to-api.md`
- M9 假设门：`prd/PRD_02_13_llm_gateway.md`
- Script manifest 范式：`docs/agenterm-rhai-runtime.md`、`agenterm.tasks.json`
- CC Intent model 选择：`plan/design-cc-hyper-control-agent.md`
