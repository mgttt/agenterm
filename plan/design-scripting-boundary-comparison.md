# 脚本/嵌入运行时边界：行业对照与 AgenTerm 定位

| 字段 | 值 |
|------|-----|
| **文档** | Lua / Python / Node / Bun 等与 AgenTerm Rhai↔Rust 边界的对照分析 |
| 日期 | 2026-08-06 |
| 状态 | 设计稿 rev1 |
| 关联 | `plan/design-rhai-rust-boundary.md`、`plan/agenterm-rhai-app.md`、`prd/PRD_02_10_rhai_scripting.md` |

---

## 1. 目的

回答两个问题：

1. **其他生态（Lua、Python、Node、Bun…）的脚本边界是否比 AgenTerm 更「贴内核」？**
2. **若是，原因是什么；AgenTerm 为何刻意把边界画在更外层？**

本文是 **产品架构对照**，不是语言性能 benchmark，也不是「该换 Rhai 为 X」的选型文。

---

## 2. 核心结论（先读）

| 命题 | 结论 |
|------|------|
| 脚本语言是否天生更贴内核？ | **否** — 边界由 **嵌入方产品** 决定，不是语言 |
| 是否存在比我们更贴内核的成功产品？ | **是** — 如 Redis+Lua、OpenResty；它们是 **另一类产品** |
| 终端/编辑器/浏览器类是否普遍更贴内核？ | **否** — VS Code、Neovim、Chrome 均 **native 内核 + 外层脚本** |
| AgenTerm 边界偏外是 Rhai 太弱吗？ | **否** — 是 **Fleet OS + 终端实时 + 证据门** 的产品选择 |
| 对我们设计的启示 | **坚持 L1 清单**；pack 做 Redis **没有** 做的那层（产品语义），不做 Redis **做了** 的那层（数据结构内核） |

---

## 3. 分析框架

### 3.1 三层（与 `design-rhai-rust-boundary.md` 对齐）

```text
L1  Kernel   — 权威、热路径、OS 机制（C/Rust/C++）
L2  Facade   — 粗粒度、有界、可审计 API（给脚本调）
L3  Script   — 产品逻辑、插件、配置、编排
```

**「贴内核」= L3 实现 L1 职责，或 L2 细到 per-byte/per-cell。**

### 3.2 比较维度

| 维度 | 问什么 |
|------|--------|
| **权威** | 脚本能否成为 live 状态唯一源？ |
| **热路径** | 脚本是否在每帧/每字节循环？ |
| **嵌入形态** | 同进程？子进程？沙箱？ |
| **热更** | 脚本/O TA 是否常见？ |
| **证据** | 发布是否绑定脚本 hash？ |

---

## 4. 行业对照表

| 生态 / 产品 | L3 语言 | 贴内核程度 | L1 典型 | L3 典型 | 备注 |
|-------------|---------|------------|---------|---------|------|
| **Redis + Lua** | Lua | **很高** | 内存数据结构、持久化、网络 | 原子逻辑、限流 | 脚本 **在 server 内** 操作数据 |
| **OpenResty / nginx + Lua** | Lua | **高** | 事件循环、连接、TLS | 路由、鉴权、改写 | 请求路径在 Lua |
| **Neovim** | Lua | **中高** | buffer/grid、UI 绘制、输入 | 插件、主题、命令 | **grid 仍在 C**；Lua 不 per-cell 解析 |
| **Emacs** | Elisp | **很高（特例）** | 大量 primitives 仍 C | 应用即 Lisp | 历史特例，难复制 |
| **游戏引擎 + Lua** | Lua | **中** | 渲染、物理、动画 | AI、UI 流程、关卡 | 热循环在 C++ |
| **VS Code** | TypeScript | **中** | Electron/native PTY、LSP 宿主 | 扩展、主题、命令 | 终端 **parser 不在 TS** |
| **Chrome / V8** | JS | **低（相对浏览器）** | Blink/V8/网络栈 | 页面逻辑 | JS **不能** 替换 layout 引擎 |
| **Node.js 应用** | JS | **视产品** | libuv、V8、openssl | 业务、中间件 | `fs/net/child_process` 像很近，**内核仍是 C++** |
| **Bun** | JS | 类似 Node | Zig/C++ 运行时 | 同 Node | 内置更多 native，**仍非全 JS 内核** |
| **CPython 应用** | Python | **视产品** | CPython VM、C 扩展 | 业务逻辑 | 数值/IO 热点在 **C 扩展** |
| **Django / Flask** | Python | **低** | DB/OS | HTTP 视图 | 典型 **胶水层** |
| **AgenTerm（目标）** | Rhai | **刻意偏外** | server、PTY、parser、blit | CC/Hub/LLM pack | 见 §2.1 内核清单 |

---

## 5. 分生态说明

### 5.1 Python：「胶水 95% 够快」从哪来？

```text
用户代码 (Python)
    ↓ 调用
C 扩展 / OS I/O  (numpy, psycopg2, ConPTY 若封装)
    ↓
内核
```

- **感觉快**：时间花在 **I/O 与 native**，不在 Python 循环。  
- **数值/网格/解析**：社区共识是 **C/Rust 写内核**，Python 写外圈。  
- **与 AgenTerm 同构**：Rhai pack ≈ Python 业务层；Rust L1 ≈ numpy/ConPTY 层。

**边界是否更贴内核？** 默认 **不**；只有 CPython **实现本身**（opcode、GC）在 C——那是 **语言 VM**，不是用户脚本贴内核。

### 5.2 Node.js / Bun：I/O 很近，内核仍 native

| API | 看起来 | 实际 |
|-----|--------|------|
| `fs.readFile` | 很近 OS | libuv → 系统调用；**不在 JS 实现磁盘** |
| `net.createServer` | 很近网络 | libuv socket；**不在 JS 实现 TCP 栈** |
| `child_process` / `node-pty` | 很近 PTY | **spawn 与 fd 在 native**；JS 读写字节流 |
| `sharp` / `better-sqlite3` | 热点 | **故意** native addon |

**终端类产品：**

- VS Code 终端：扩展 TS + **`node-pty`（native）** + **xterm.js（Wasm/JS 渲染已有 grid）** — 仍 **不是** 用 JS 重写 ConPTY。  
- 若用 Node 写完整终端 **产品**，严肃方案仍是 **native parser + 可选 JS chrome**，与 AgenTerm 分层一致。

**Bun** 把更多标准库做成 native/Zig，是 **L2 更厚、更快**，不是让用户 JS 替换 PTY 内核。

**边界是否更贴内核？** **API 表面更近 OS**；**架构上 L1 仍在 native**。AgenTerm 的 `fleet.*` 是 **显式 L2**，不假装脚本「就是 server」。

### 5.3 Lua：嵌入专用，跨度最大

| 用法 | 贴内核 | 例子 |
|------|--------|------|
| 配置 DSL | 低 | `--lua` 几行配置 |
| 游戏逻辑 | 中 | WoW、Love2D（仍 C 渲染） |
| 请求/数据路径 | 高 | OpenResty、Redis |
| 整个应用 | 极高 | Neovim（但 buffer 在 C） |

Lua **语言** 不负责边界；**嵌入方** 决定暴露哪些 C API。

**对 AgenTerm 的启示：** 若未来要做 **「Fleet 内原子脚本」**（类似 Redis），可 **单独** 做 **受限子集 + 同进程 sandbox**——那是 **新 PRD 模块**，不是把现有 unrestricted Rhai pack 塞进 server 进程。

### 5.4 Neovim / Emacs（编辑器对照）

| | Neovim | AgenTerm |
|--|--------|----------|
| 网格/blink | C core | Rust `terminal_runtime` |
| 插件语言 | Lua | Rhai pack（产品层） |
| 插件能否改 grid 语义 | 通过 **C API**，非纯 Lua 重写 parser | **禁止** pack 实现 parser |
| 热更插件 | 常见 | pack reload（计划） |

Neovim 的 Lua **比我们的 pack 更贴 core**，但仍 **不能** 用 Lua 替换底层 buffer 实现——与 **§2.1 域 D** 同类。

Emacs 是 **反例**：Elisp 即产品；维护/性能代价大，**非本产品的目标形态**。

### 5.5 Redis + Lua（最贴内核的一类）

- 脚本与 **数据面同进程**、操作 **内置数据结构**。  
- 适合：**短、原子、可预测** 的服务端逻辑。  
- **不适合：** 长生命周期 GUI、复杂 PTY 树、人类交互焦点、4 MiB PE 预算。

AgenTerm **不应** 对标 Redis 脚本模型做默认 pack；Fleet 权威与 GUI 证据门冲突。

---

## 6. 为何 AgenTerm 边界画在更外层

| 因素 | 说明 |
|------|------|
| **产品类型** | 本地 **Fleet 终端 OS**，不是通用 app server |
| **权威 invariant** | server epoch/journal/receipt；pack 可换 |
| **实时性** | cell 级热路径；脚本 VM 跨界成本高 |
| **证据门** | L1 随 Base Candidate 封印；L3 pack 独立 hash |
| **Rhai 哲学** | unrestricted runtime；边界靠 **Facade + 架构**，不靠 profile 阉割 |
| **跨平台** | ConPTY / winit / Win32 已在 Rust 机制层；脚本统一 **语义** 即可 |

这不是「Rhai 不如 Lua/JS」，而是 **刻意不学 Redis 脚本进内核**，也 **不学 Emacs 全应用脚本化**。

---

## 6.1 假设：若 Rhai / App Pack 有 JIT，会更贴内核吗？

**短答：性能上会更「敢写」L3 逻辑；架构上 **不会** 自动获得接管 L1 的正当性。**

### JIT 会改变的

| 方面 | 效果 |
|------|------|
| **L3 计算** | 路由、状态机、字符串/layout 生成 **更快** → pack 可 **更大** |
| **L2 调用频率** | 仍跨界，但 **单次 pack 内** 循环可 JIT，减少解释开销 |
| **Strangler 深度** | 更多 **产品语义** 从 Rust 迁 pack **更可行**（仍 T3，非 T0） |
| **与 LuaJIT/V8 类比** | 游戏 **AI/规则** 更厚；**不是** 渲染进脚本 |

### JIT 不会改变的

| 方面 | 原因 |
|------|------|
| **Fleet 权威** | epoch/journal/receipt 仍在 **server**；JIT 不产生单一真相 |
| **PTY/ConPTY** | OS 句柄与泵在 **native**；JIT 不能替代驱动 |
| **Parser/grid 是否进 pack** | 瓶颈是 **数据布局 + 跨界 + 证据**，不是仅「解释慢」 |
| **安全/热更信任** | OTA pack 仍要签名；JIT 代码 **同样** 不可 silent 进 server 进程 |
| **Candidate 封印** | L1 Base 仍随 PE 发版；JIT 不改变 **何者随 pack 热更** |
| **unrestricted Rhai** | JIT 不是 sandbox；**不能** 因「Compiled」就放宽 B1 |

### 可能 **错误** 推导（需禁止）

```text
❌ 「JIT 后 parser 可以写 pack 里，够快了」
   → 仍违反 B1/B3/B5；qualification 组合爆炸

❌ 「JIT pack 可进 agenterm server 同进程当内核插件」
   → 那是 Redis 模型；默认 **非目标**（见 BC-3）

❌ 「JIT = 95% 原生，所以 L1/L3 边界可模糊」
   → PyPy 也快，numpy 仍在 C；边界是 **职责** 不是 **MFLOPS**
```

### 合理推导（若投资 JIT）

| 可做更多（L3↑） | 仍不做（L1） |
|----------------|--------------|
| CC 整屏 lines 生成、复杂 nav 状态机 | `terminal_runtime` / parser |
| Hub 路由、LLM 映射、主题规则 | PTY 泵、blit、IPC 帧 |
| 用户 automation 更重（非 pack） | tab 树、journal 实现 |

### 与行业 JIT 对照

| 运行时 | JIT | 内核仍在 native？ |
|--------|-----|------------------|
| **LuaJIT** | 是 | 是（渲染/物理 C++） |
| **PyPy** | 是 | 是（numpy/C 扩展） |
| **V8** | 是 | 是（Blink/libuv） |
| **JIT Rhai（假设）** | — | **仍是**（§2.1 清单不变） |

**结论：** JIT 是 **L3 能力放大器**，不是 **L1 迁移许可证**。是否投资 JIT 应单独做 **成本/收益**（Engine 复杂度、AOT 缓存、调试）；**不应** 作为「把内核搬进 pack」的前提。

---

## 7. 可借鉴 vs 不借鉴

| 来源 | 可借鉴 | 不借鉴 |
|------|--------|--------|
| **Python** | 胶水 + C 扩展分层；I/O bound 脚本够快 | 指望脚本层 95% 原生覆盖 **grid/parser** |
| **Node/Bun** | 清晰 L2（libuv API）；native addon 做热点 | `child_process` 当第二 Fleet 权威 |
| **Lua 嵌入** | 小 pack、热 reload、同 catalog 发现 | Redis 式 **server 内** 操纵核心数据结构 |
| **Neovim** | 插件改 **行为/命令/主题**；core API 稳定 | 插件重写 buffer/parser |
| **VS Code** | 扩展改 UI/命令；PTY **native** | 扩展实现终端模拟器内核 |
| **Redis** | 有界脚本、原子执行、明确禁止项 | 把 Rhai pack 放进 server 进程当默认 |

---

## 8. 与 AgenTerm 文档映射

| 行业说法 | AgenTerm 文档 |
|----------|----------------|
| C extension / native addon | L1 内核 §2.1 + `agenterm-platform` |
| Python 业务 / JS 应用代码 | L3 Rhai App Pack |
| Node `fs`/`net` 式 API | L2 `fleet.*` / `std.*` / 未来 `product.*` |
| npm 热更包 | sealed `.agp` pack（非 npm 模型，见 `design-release-base-vs-apps.md` §4.4） |
| Redis 脚本进内核 | **非目标**；若要做 = 新模块单独立项 |

---

## 9. 开放问题（BC-*）

| ID | 问题 |
|----|------|
| BC-1 | 是否需要 **Neovim 式**「用户插件 Rhai」与 **产品 pack** 分 channel？ |
| BC-2 | 是否研究 **xterm.js 式** grid 在 Wasm（L1 仍 native blit）？ |
| BC-3 | Fleet 内 **短脚本原子 op**（Redis 类）是否单独 PRD，默认 pack 禁止？ |
| BC-4 | 是否投资 Rhai/AOT/JIT 仅放大 L3，**明确不** 作为 L1 迁移理由？ |

---

## 10. 交叉引用

- L1 内核清单：`plan/design-rhai-rust-boundary.md` §2.1  
- App Pack / Strangler：`plan/agenterm-rhai-app.md`  
- 发布分轨：`plan/design-release-base-vs-apps.md`  
- Script 契约：`prd/PRD_02_10_rhai_scripting.md`  
- 架构三层：`plan/ARCHITECTURE.md` §1.0  

---

## 11. 摘要（评审用）

**其他生态并非天然更贴内核**；Redis/Lua、OpenResty **是**，VS Code/Neovim/Chrome **否**。  
Node/Bun **API 离 OS 近**，但 **V8/libuv 仍是内核**。  
AgenTerm 选择 **外层 pack + Rust L1**，与 **严肃终端/编辑器** 同族，与 **Redis** 不同族——这是 **产品定位**，不是 Rhai 能力上限。
