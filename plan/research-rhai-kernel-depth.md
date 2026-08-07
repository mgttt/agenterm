# Rhai 向内核「挤压」深度：研究上限图

| 字段 | 值 |
|------|-----|
| **文档** | 在不替代 L1 前提下，Rhai/pack **最多能挤多深** 的探索图 |
| 日期 | 2026-08-06 |
| 状态 | 研究稿 rev1（非承诺路线图） |
| 关联 | `plan/design-rhai-rust-boundary.md`、`plan/agenterm-rhai-app.md`、`plan/design-scripting-boundary-comparison.md` §6.1 |

---

## 1. 问题

**不是**「用 Rhai 替换内核」，而是：**黑科技/产品欲界内，Facade + pack 能把脚本能力推到离 L1 多近？硬顶在哪？**

本文给出 **深度阶梯 D0–D9**、每项增益/风险/证据成本，以及 **不可逾越的硬顶 H1–H5**。

---

## 2. 深度阶梯（D0 = 最外，D9 = 最贴内核但仍非 L1）

```text
D0 ────────────────────────────────────────────────► D9 ──X── H 硬顶
外圈产品文案                              贴内核 Facade/同进程钩子        禁止替代 L1
```

| 深度 | 名称 | Rhai 做什么 | 仍留在 Rust L1 什么 | 产品价值 |
|------|------|-------------|---------------------|----------|
| **D0** | 占位 / 元数据 | `pack.version`、about 字符串 | 全部 | 生命周期验证 |
| **D1** | 静态配置 | reason 映射表、i18n、feature flags | 全部行为 | 热更文案/开关 |
| **D2** | 冷路径编排 | 启动时注册路由；LLM provider 表 | 帧循环、server | 网关/CC 策略 |
| **D3** | 温路径呈现 | CC 每帧 `present_lines()`；nav 状态机 | blit、hit-test 实现 | 跨平台 UX 对齐 |
| **D4** | In-process Facade | CC 内嵌 Engine；**不**走 broker IPC | server 进程仍独立 | 降延迟、可每帧调 T3 |
| **D5** | 事件订阅 | pack 收 `fleet.events` 流；触发 **建议** 不 **突变** | journal 写入、receipt | 超控台、告警规则 |
| **D6** | 有界流变换 | PTY 字节流 **filter**（max N KiB/s）；不进 parser | parser、grid 所有权 | 自定义 log 着色/过滤 |
| **D7** | 注册式 escape 扩展 | 少量 **私有** CSI/OSC 由 pack 解释 → **Facade 回写** grid | VT 状态机主体 | 企业定制序列（极高风险） |
| **D8** | Server 同进程 **只读** 钩子 | Rhai 读共享只读 snapshot（无 broker） | 所有 **mutation** 路径 | 极低延迟只读仪表 |
| **D9** | Server 同进程 **原子子集** | Redis 式：短脚本、无 GC 压力、固定 API 子集 | PTY/parser/blit；完整 Rhai **不** 进 server | Fleet 内规则引擎（新 PRD） |

**H 硬顶（§4）：** D9 以上 = 改写 L1 实现或第二权威 → **默认禁止**。

---

## 3. 挤压手段清单（按「能多深」排序）

### 3.1 工程手段（不改产品语义）

| 手段 | 典型深度 | 说明 |
|------|----------|------|
| **AOT 字节码缓存** | D2–D4 | 冷启动与 reload 更快；**不** 加深语义 |
| **JIT / tiered compilation** | D3–D5 | L3 更厚；见 `design-scripting-boundary-comparison.md` §6.1 |
| **In-process Engine** | D3–D4 | CC/gateway 内嵌；eliminate broker  on hot path |
| **Batch Facade** | D4–D6 | 一次调 `capture_range`/`apply_lines` 代替 N 次细调 |
| **WASM 模块（pack 附带）** | D4–D6 | 重计算在 wasm；Rhai 编排；blit 仍 Rust |
| **专用字节码 DSL** | D6–D7 | 非 Turing-complete 的 filter 语言 → 比 Rhai 更安全地贴流 |

### 3.2 Facade 加深（扩 L2，不扩 L1 实现）

| 新 Facade（示意） | 深度 | 增益 | 必须有的护栏 |
|-------------------|------|------|--------------|
| `product.cc.present_frame(spec)` | D3 | 整帧语义 | budget lines/bytes；fallback |
| `fleet.events.subscribe(filter)` | D5 | 实时规则 | 只读；filter 有界；无直接 mutate |
| `terminal.stream.filter(fn)` | D6 | 字节流处理 | max throughput；不可 hold 无限 buffer |
| `terminal.escape.register(range, handler)` | D7 | 定制序列 | allowlist；handler 不可 block；audit |
| `fleet.snapshot.ro()` in-process | D8 | 零拷贝读 | 只读映射；epoch 代数失效 |

**规则：** 每加深一级 Facade，catalog 增 **Tier + budget + 独立 smoke**；D6+ 需 **主控 + 安全 review**。

### 3.3 进程/部署挤压

| 布局 | 深度 | 说明 |
|------|------|------|
| pack 在 CC 进程 | D3–D4 | 已计划 |
| pack 在 gateway 进程 | D2–D5 | LLM 线 |
| **只读** Rhai 在 server 进程 | D8 | 与 GUI 解耦的观察；**mutation 仍 IPC** |
| **原子子集** Rhai/Lua 在 server | D9 | 新模块；**不是** unrestricted `agenterm-rhai` |

---

## 4. 硬顶 H1–H5（再挤也不可越过）

| ID | 硬顶 | 原因 |
|----|------|------|
| **H1** | Rhai **实现** parser/grid/scrollback 状态机 | cell 热路径 + 证据 + 数据布局 |
| **H2** | pack 成为 Fleet **live 权威**（缓存 tab 树不校验 epoch） | 第二真相 |
| **H3** | unrestricted Rhai **默认**进 `agenterm server` 同进程 | AGENTS.md；攻击面 = server |
| **H4** | 无签名 OTA 改 D6+ Facade 行为 | 等同 RCE |
| **H5** | per-byte Rhai 回调进 parser 主循环 | 跨界 × N；JIT 也救不了架构 |

**「黑科技」可以碰 D6–D9 的 research spike，不能 silent 进默认 SKU。**

---

## 5. 与 Redis/LuaJIT 的对照：我们能挤到多深？

| 生态 | 典型深度 | AgenTerm 可对齐？ |
|------|----------|-------------------|
| Redis Lua | D9（数据面原子脚本） | **可选** 单列 `fleet.script.atomic` PRD，**非**默认 pack |
| LuaJIT 游戏 | D4–D5 规则 + C 渲染 | **默认目标** D3–D4（CC pack） |
| Neovim Lua | D3–D5 + C API | D3–D5 可对标；**grid 仍 Rust** |
| Node node-pty | D6 字节读写 | D6 **filter** 可研究；parser 仍 Rust |

**务实「挤」的目标带：默认 SKU **D3–D4**；research **D5–D6**；D7+ 需 explicit 立项。**

---

## 6. 推荐研究 spike（由浅入深）

| 优先级 | Spike | 深度 | 产出 |
|--------|-------|------|------|
| P0 | 占位 pack + reload | D0 | 已有 plan |
| P1 | CC `present_lines` in-process | D3 | Facade + smoke |
| P2 | gateway pack + `llm.*` | D2–D4 | 已有 plan |
| P3 | `fleet.events` 订阅（低频） | D5 | 超控规则原型 |
| P4 | PTY 输出 **有界** filter（Rhai 或 micro-DSL） | D6 | research/pty-filter |
| P5 | AOT pack 字节码 | D2–D4 | 启动性能 |
| P6 | server RO snapshot 钩子 | D8 | 延迟仪表；**只读** |
| P7 | escape 注册（私有 OSC） | D7 | 企业定制；高风险 |
| P8 | server 内原子脚本子集 | D9 | 新 PRD；Redis 类 |

---

## 7. 深度 vs 风险矩阵

```text
风险 ↑
  │     D7 escape    D9 atomic
  │          D6 filter
  │     D5 events
  │  D3 CC frame
  │ D0–D2 config/LLM
  └────────────────────────────► 深度
        低                     高

默认发货区：D0–D4
实验室：D5–D6
需新 PRD+安全门：D7–D9
```

---

## 8. 开放问题（RK-*）

| ID | 问题 |
|----|------|
| RK-1 | D6 filter 用 Rhai 还是 **非图灵** DSL？ |
| RK-2 | D8 RO 钩子是否 worth server 进程复杂度？ |
| RK-3 | D9 是否与 unrestricted Rhai **品牌隔离**（另解释器名）？ |
| RK-4 | WASM 在 D4–D6 的首个场景：layout 还是 filter？ |

---

## 9. 交叉引用

- L1 清单：`plan/design-rhai-rust-boundary.md` §2.1  
- 边界八条：`plan/design-rhai-rust-boundary.md` §3  
- JIT：`plan/design-scripting-boundary-comparison.md` §6.1  
- App Pack：`plan/agenterm-rhai-app.md`  

---

## 10. 摘要

**能挤多深？** 默认产品 **D3–D4**（CC 整帧呈现 + in-process）；research **D5–D6**（事件、有界流过滤）；**D7–D9** 需单列立项且多数 **不进默认 server**。  
**硬顶：** parser/grid 实现、Fleet 权威、unrestricted 脚本默认进 server、per-byte 主循环回调。  
**黑科技欲望** 应花在 **更厚的 L2 Facade + in-process + 有界流**，不是 **把 L1 改名成 pack**。

---

## 11. 编译「多原生」轴（与深度正交）

用户常把问题归结为：**编译器能把 Rhai 编译到多原生，就能挤多深？**  
**半对：** 原生度决定 **哪些深度档位「跑得动」**；**不决定** H1–H5 硬顶是否可越过。

### 11.1 两根轴

```text
                    架构深度 D0 ──────────────────► D9
                    │
  编译原生度         │   D3 整帧 present
  （执行效率）       │        D6 流 filter
       ↑            │             D7 escape hook
       │            │
  AOT / JIT         │   「同样深度，原生度越高越可上线」
       │            │
  解释执行           │   D0–D2 永远够；D3 勉强；D6+ 不现实
       └────────────┴──────────────────────────────►
                              H 硬顶（与编译无关）
```

| 轴 | 问什么 | 谁决定 |
|----|--------|--------|
| **深度 D** | 脚本 **管什么职责** | 产品 + B1–B8 |
| **原生度 N** | 脚本 **跑多快** | 编译/VM 工程 |
| **硬顶 H** | **什么永远不能** | Fleet/终端 invariant |

### 11.2 Rhai 现状（2026）

| 层级 | 状态 |
|------|------|
| **上游 Rhai** | 以 **AST 解释** 为主；无官方生产级 JIT/AOT |
| **AgenTerm** | `script_runtime` 宿主 + catalog；pack 字节码缓存 **未** 产品化 |
| **跨界成本** | 即使 Rhai 零开销，**L2 Facade FFI**（Dynamic、调用注册函数）仍有成本 |

故：**不投资编译器** → 务实深度 **D3 上限**（每帧少量 Rhai + 大量 Rust fallback）。  
**投资编译器** → **同一深度** 更稳，且 **可能** 把 **D6 有界 filter** 从 research 拉到 SKU。

### 11.3 编译路线（由低到高原生度）

| 路线 | 原生度 | 工程 | 适用深度 | 备注 |
|------|--------|------|----------|------|
| **N0 解释** | 基线 | 已有 | D0–D3（轻） | 默认 |
| **N1 AST 常量折叠 + 编译缓存** | 低+ | 中 | D2–D3 | pack reload 快；**不** 加快热循环 much |
| **N2 字节码 + 栈 VM** | 中 | 大 | D3–D4 | 自研或 fork；Rhai 需改或子集 |
| **N3 基础块 JIT（Cranelift/LLVM）** | 高 | 很大 | D4–D6 | 仅 **Tier-1 纯函数**；host 调用仍边界 |
| **N4 AOT → .so / 静态 Rust** | 很高 | 很大 | D3–D5 稳定路径 | 发布时编译 pack；**热更** 变 re-AOT |
| **N5 子集 transpile → Rust** | 最高（包内） | 大 | D2–D4 | pack 语言受限；dogfood 编译器 |
| **N6 WASM in pack** |  compute 高 | 中 | D4–D6 算子 | Rhai 编排；filter/layout 热点 wasm |
| **N7 Rhai 调 Rust 预编译 hook** | 原生 | 小 | D3+ | 热点函数 **手工** Rust；pack 只配表 |

**没有银弹：** N3–N5 都 **不解** host Facade 边界；JIT 再快，`fleet.mutate` 仍是一次 L2 旅行。

### 11.4 深度 × 原生度：什么变得「可行」

| 深度 | N0 解释 | N3 JIT / N6 WASM | 仍不可行（任意 N） |
|------|---------|-------------------|---------------------|
| D3 CC 整帧 | 轻量行 OK；复杂 layout 需 Rust | 复杂 layout、nav 状态机可 pack | blit/parser |
| D5 事件规则 | 低频 OK | 高频 filter 规则 OK | journal 写入权 |
| D6 流 filter | 通常 **不够** | **可能**（有界 buffer + 编译循环） | 拥有 grid |
| D7 escape | 风险高 + 慢 | 快但仍 **极高风险** | 替代 VT 状态机 |
| D8–D9 | 无关 | RO 钩子仍靠 **内存模型** 设计 | mutation 进脚本 |

### 11.5 与「Python/C 扩展」类比

```text
编译原生度 ↑  ≈  「更多 pack 逻辑像 Python，更少像 numpy 内层必须 C」

但 numpy 内层仍 C  ⇔  parser/PTY/server 仍 Rust（与 N 无关）
```

**挤深度的杠杆排序（诚实）：**

1. **架构**：in-process Facade、batch API（**免费**增益）  
2. **深度选择**：只挤 D3–D5，不碰 D7+  
3. **编译**：N1 缓存 → N6 WASM 热点 → N3 JIT（按 ROI）  
4. **手工 Rust hook**（N7）：最热 5% 永远 native  

### 11.6 开放问题（RC-*）

| ID | 问题 |
|----|------|
| RC-1 | pack 首版目标 **N0 还是 N1**（字节码缓存）？ |
| RC-2 | D6 filter 用 **N6 WASM** 还是 **N3 JIT 子集**？ |
| RC-3 | 是否 **限制 product pack 语言**（无 eval）换 N4/AOT 可预测性？ |
| RC-4 | 编译产物是否进 **qualification hash**（与 Base 分离）？ |

### 11.7 摘要

**「能挤多深」≈ min(架构允许深度 D, 编译跑得动的 D)。**  
编译器 **抬高 D3–D6 的实用上限**，**不抬高 H 硬顶**。  
当前 Rhai 解释执行：**先把 D3–D4 做稳**；若黑科技要继续挤，**优先 N1 缓存 + N6 WASM 算子**，再谈 JIT——而不是假设「全原生 Rhai = 可以接管 parser」。

