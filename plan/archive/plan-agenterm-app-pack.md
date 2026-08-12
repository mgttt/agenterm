# ⚠️ 已归档：agenterm.app 脚本应用包独立计划书

> **归档于 2026-08-10。** 本文的生效架构合同、v0.1.18 Phase 0 执行叶、风险与
> 后续 Phase 去向已收敛到 [`../plan-v0.1.18.md`](../plan-v0.1.18.md)。本文只保留
> 早期方案比较与完整推演历史，**不是执行依据或现行 SSOT**；不得从本文单独恢复
> Rh App 自动回退、全量 surface 对齐或超出 v0.1.18 Gate 的范围。

# agenterm.app：脚本应用包独立计划书（历史原稿）

| 字段 | 值 |
|------|-----|
| **文档** | 脚本应用包的完整分期计划——自解包、双引擎策略、Strangler 渐进迁移 |
| **日期** | 2026-08-10（rev2：事实校正 + 引擎默认化前置 + fallback 两段式） |
| **状态** | 定稿，待 v0.1.17 收口后授权开工 |
| **前置** | `plan/agenterm-rhai-app.md`（架构讨论稿 rev1）、`plan/design-rhai-rust-boundary.md`（边界 SSOT）、`plan/design-release-base-vs-apps.md`（发布分轨）、`plan/design-scripting-boundary-comparison.md`（引擎边界对照）、`plan/ARCHITECTURE.md`（结构 SSOT） |
| **产品归属** | `prd/PRD_02_10_rhai_scripting.md` §Layered deployment、`prd/PRD_02_02_executable_family.md` |
| **决策人** | 产品范围决策；引擎选型在 §7，决策表在 §9 |

---

## 1. 一句话

**把 agenterm 的产品行为从 Rust 源码逐步迁移到密封的脚本应用包（`agenterm.app`），**
**运行时自解包、可独立更新；PTY/渲染/Server 内核永久 native。**
**CI 成本：Base 全矩阵 6 格编译保留给内核变更；OTA 应用层更新 = 上传一个 .agp 文件。**

---

## 2. 问题与动机（树）

```text
为什么做（根）
├── 2.1 Base 发布太贵、太慢
│   ├── Release Candidate 需全矩阵 6 格编译：`{x86_64, aarch64} × {win, lnx, osx}`
│   ├── Windows stress qualification 需真实 ConPTY（Wine 无法模拟）
│   ├── 改一句空态文案 → 重跑全平台 gate → 等待 30–60 分钟 CI 墙钟
│   ├── Base 每发一版 = 6 个平台 zip + macOS 签名/公证 + Windows 签名
│   ├── 用户为一个 CC 文案修复等 0.1.x → 0.1.y 整版
│   └── OTA 更新 = 上传一个 .agp → 发布 CI 零成本（只是静态文件）
│       └── ⚠️ 仅对 OTA 成立：内嵌出厂包随 Base 走，照吃全矩阵
│
├── 2.2 跨平台 UX 漂移
│   ├── Win/Unix 各有一套 Rust UI 代码（`windows/remote_frontend.rs` vs `unix/frontend/`）
│   ├── 产品语义双写 → 行为不一致 bug 难以根治
│   └── 同一套脚本 → 语义单点 → 只维护 native 渲染差异
│
├── 2.3 进化速度与内核速度不匹配
│   ├── CC 导航 / LLM 路由 / Hub 策略 → 周级迭代
│   ├── PTY / ConPTY / 协议 / parser → 月级迭代
│   └── 绑在一个 semver 里 = 慢的拖累快的
│
└── 2.4 已有成熟基础无需从零建设
    ├── rh AOT pack 已是生产级（`agenterm-rh/pack.rs`：build_pack_dir→load→entry_value→cc_lines）
    ├── QJS pack 已对接（`agenterm-qjs/pack.rs`：`QjsPack::load(dir)` → `verify_*` → `QjsPack::eval(&host)`）
    ├── 跨引擎共享层已就绪（`agenterm-script-common/pack_support.rs`）
    ├── in-process loader 有先例（`script_rh_pack.rs`：cached_rh_pack + try_load_rh_pack_from_env）
    └── ⚠️ 但 QJS 目前是 opt-in feature（根 `Cargo.toml`：`default = []`，`script-qjs` 可选）
        └── 内嵌到 Base 需先把它转默认，代价见 §7.2 —— 这是 Phase 0 的真前置
```

---

## 3. 目标形态（树）

```text
产物
├── 3.1 agenterm 主程序（Thin Base）
│   ├── 包含：Server · PTY · parser · 渲染 · IPC · 协议 · Fleet 权威
│   ├── 包含：Script Engine 宿主（内嵌 Engine、pack 加载器、自解包逻辑）
│   ├── 包含：签名验证 + 更新通道基础设施
│   ├── 内嵌：一个出厂 .agp 归档（编译时 `include_bytes!` 进 PE 资源段）
│   └── 不包含：产品行为策略、CC 导航、空态文案、LLM 路由、主题应用逻辑
│
├── 3.2 agenterm.app（脚本应用包）
│   ├── 路径：`<平台数据目录>/agenterm/app-pack/`（解压后运行副本）
│   │   ├── 一律经 `src/platform/policy/paths.rs::local_data_root_for_product_directory()`
│   │   ├── Windows → `%LOCALAPPDATA%\agenterm\app-pack\`
│   │   ├── Linux → `~/.local/share/agenterm/app-pack/`
│   │   └── macOS → 按同一 policy 解析；**计划全文不得硬编码 `~/.local/share`**
│   ├── 格式：密封目录，含 `manifest.json` + entry 脚本 + 子模块
│   ├── 内容（渐进）：CC 导航 → 空态文案 → hyper_control 分区 → LLM 路由 → toolbar 策略
│   ├── 更新：独立 channel，签名 + 显式用户确认 + 回滚
│   └── 来源：出厂内嵌 → 首次启动解压 → 后续远程更新覆盖
│
└── 3.3 启动流程
    ├── ① agenterm.exe 启动
    ├── ② 读 `<数据目录>/agenterm/app-pack/manifest.json`（缺失 → 视为"无 pack"）
    ├── ③ 与内嵌出厂包比对 —— **三态判定，不是"存在即跳过"**
    │   ├── 无 pack → 解出出厂包，标 origin=factory
    │   ├── origin=factory 且 factory.app_version > local → 覆盖升级（用户没改过，安全）
    │   ├── origin=user（用户改过，本地 sha256 ≠ 落盘时记录的 sha256）
    │   │   └── 不覆盖；日志 + `app-pack doctor` 提示可 `--force` 或 `factory-reset`
    │   └── origin=ota（远程更新装的）→ 只跟 channel 比，出厂包不回滚它
    ├── ④ 加载 manifest + entry → 长驻 Engine（One Engine Per Process）
    ├── ⑤ Native 通过 catalog Facade 调用 pack 获取产品语义
    └── ⑥ （可选）后台嗅探远程更新 → 用户确认 → 下载 → staging → reload

    ③ 的 origin + 落盘 sha256 记在 pack 目录旁的 `app-pack.state.json`（不在 pack 内，
    否则用户改 pack 会连状态一起改掉）。没有这个三态，出厂包在 Base 升级后永远升不上去，
    或者反过来把用户的 fork 静默覆盖 —— 而 §6.1 是明确鼓励用户改本地文件的。

3.4 命名约定：三层、三个名字

    "agenterm.app"（产品概念）
        "agenterm.agp"（文件格式）
            "app-pack/"（运行时目录）

    这三个名字指同一件事的三个形态，不是三个不同的东西：

    ┌─────────────────────────────────────────────────────────────┐
    │ 概念层    agenterm.app      "脚本应用包"这个产品概念           │
    │                             用于：文档、对话、README          │
    ├─────────────────────────────────────────────────────────────┤
    │ 文件层    .agp 文件          tar.zst 密封归档                  │
    │                             agenterm-app-0.3.1.agp           │
    │                             用于：下载、分发、内嵌到 PE        │
    ├─────────────────────────────────────────────────────────────┤
    │ 运行时    app-pack/ 目录     解压后的源码文件树                │
    │           <数据目录>/       用于：Engine 加载、用户检视/修改   │
    │           agenterm/app-pack/  平台差异见 §3.2                 │
    └─────────────────────────────────────────────────────────────┘

    为什么文件扩展名是 .agp 而不是 .app：

    ├── macOS 上 .app 是应用程序包（Safari.app、Terminal.app）
    │   └── Finder 将 .app 目录当可执行程序处理，双击会尝试启动
    ├── .agp = Agenterm aGp Pack — 避免 OS 层面的混淆
    ├── Windows/Linux 上 .app 无特殊含义，但为了三端一致，统一用 .agp
    └── 类比：VS Code 扩展 = .vsix 文件（不是 .vscode 也不是 .app）

    在本文档中的写法约定：

    ├── 讨论产品概念时：写 "agenterm.app" 或 "app pack"
    ├── 引用文件时：写 `agenterm-app-0.3.1.agp`
    ├── 引用路径时：写 `<数据目录>/agenterm/app-pack/`（不写具体平台路径，见 3.2）
    └── 代码中的常量/标识符：`APP_PACK_VERSION`、`app_pack_version`（snapshot 字段）
```

---

## 4. 不变量与禁令

| # | 规则 | 理由 |
|---|------|------|
| I1 | **PTY / ConPTY / parser / 渲染 blit 永不脚本化** | 字节级热路径；60fps 像素管线 |
| I2 | **Server / Fleet 权威永不脚本化** | 唯一真相源；tab 树 / workspace / journal |
| I3 | **IPC 传输 / 协议永不脚本化** | 机制，非产品策略 |
| I4 | **pack 不得缓存 Fleet 状态** | 第二权威风险；pack 只投影 server snapshot |
| I5 | **pack 失败 → 永远有兜底，但兜底语义分两段** | 见下方 I5 细则；"等价 Rust 实现"和"迁完删 Rust"不可能同时成立 |
| I6 | **pack 不做 permission sandbox** | 审批/配额仍在 Agent harness/native |
| I7 | **pack 不做 npm 依赖树** | 整包替换密封目录；不传递求解 |
| I8 | **不新增第二套 rh runtime / host API** | 复用现有 `fleet.*` / `std.*` / catalog；增量 `product.*` |
| I9 | **不静默远程替换** | 签名 + 显式用户确认 + 回滚 |
| I10 | **不新增独立 PE** | pack 是数据，不是进程；Engine 内嵌 |
| I11 | **签名通过的 pack ≡ 用户权限下的任意代码执行** | pack 能调全量 `fleet.*`（含 destructive）；I6 说了没有 sandbox，所以**签名是唯一边界** |
| I12 | **pack 连续失败 N 次 → 自动禁用并提示** | 每帧都走崩溃路径不可接受；见 §10.8 |

**I5 细则（两段式兜底）**

| 阶段 | 兜底语义 | 理由 |
|---|---|---|
| Phase 0–1 | **Rust 等价实现**：pack 缺失/失败时行为与 pack 完全一致 | 此时 Rust 分支还在，双路径短存是刻意的 |
| Phase 2 起 | **最小安全态**：功能降级（空 CC + 一行诊断），**不是**等价 CC | 迁移的定义就是删掉 Rust 重复；要求等价 = 要求永不删，与 §8.2.3 直接矛盾 |

进入 Phase 2 时必须同步更新证据门：Phase 0–1 的 fallback 门断言"内容相同"，
Phase 2 起的 fallback 门断言"降级态可用且可诊断"，而不是"内容相同"。

---

## 5. 已有基础设施（盘点）

```text
✅ 可直接复用
├── 5.1 rh Pack（agenterm-rh）
│   ├── crate: `crates/agenterm-rh/`
│   ├── pack.rs: build_pack_dir(source, dir) → PackBuildOutput
│   ├── pack.rs: RhPack::load(path) → entry_value / cc_lines / api_version
│   ├── manifest.rs: RhPackManifest { schema, rh_version, source_hash, native_hash, native_file, entry_symbol, cc_line_count }
│   ├── compile.rs: compile_native(source, path) → AOT .dll/.so
│   ├── bundle.rs: bundle_project_source(root, source) → 展平 import 图
│   └── 当前用法：环境变量 `AGENTERM_RH_PACK` → `script_rh_pack.rs` in-process loader
│
├── 5.2 QJS Pack（agenterm-qjs）
│   ├── crate: `crates/agenterm-qjs/`
│   ├── pack.rs: QjsPack::load(dir) → { root, manifest, source }
│   ├── pack.rs: QjsPack::eval(&QjsHostFunctions) → EvalOutcome  ← pack 的求值入口
│   ├── pack.rs: build_pack_dir(source, dir) → bytecode hash + manifest
│   ├── manifest.rs: QjsPackManifest { schema, version, source_hash, bytecode_hash, bytecode_file, entry_file }
│   │   └── + verify_bytecode(dir) / verify_source(dir) / read / parse / write
│   ├── compile.rs: compile_qjs(source, label) → (bytecode, hash)
│   ├── eval.rs: eval_entry_with_host(source, label, host) ← **裸源码**求值，不是 pack 路径
│   ├── gen_module: `scripts/qjs/lib/fleet.js`（209 行，与 rh fleet 语义对齐）
│   └── 当前用法：CLI `agenterm qjs pack` / `check` / `eval`
│       └── ⚠️ 整个 crate 由 `script-qjs` feature 门控，**默认不编译**
│
├── 5.3 跨引擎共享层（agenterm-script-common）
│   ├── crate: `crates/agenterm-script-common/`
│   ├── pack_support.rs: verify_file_hash / hash_source / write_json_receipt / read_json_receipt
│   ├── check_many.rs: 批量 `check` 的 manifest 驱动
│   └── hex.rs: sha256_hex
│
├── 5.4 产品层 glue
│   ├── script_rh_pack.rs: cached_rh_pack() / try_load_rh_pack_from_env()
│   ├── script_backend.rs: ScriptBackend 枚举（Rh/Lua/Qjs/Sql）+ from_entry_path
│   ├── script_engine.rs: ScriptEngineBackend trait + static dispatch
│   ├── script_rh_host.rs: FleetBridgeFn → fleet_call(operation_id, params_json)
│   ├── script_qjs_host.rs: QjsHostFunctions → 同操作目录
│   └── src/frontend/*: 产品语义已单点化（CC nav、settings modal、tab editor…）

⚠️ 缺口（本计划要补的）
├── 5.5 Pack 生命周期
│   ├── 自解包：主程序 PE 资源段内嵌 → 首次启动解到用户目录
│   ├── 三态 origin 判定 + `app-pack.state.json`（见 §3.3 ③）
│   ├── pack.version() / pack.reload() → catalog 新 surface
│   └── CLI: `agenterm cli app-pack status|reload|doctor|factory-reset`
│
├── 5.6 嵌入模式
│   ├── 长驻 Engine（不是跑完即退出的 task）
│   ├── Engine init 在 server 进程启动时，reload 不杀 PTY
│   ├── 多窗口共享一个 Engine（不是每窗一个）
│   └── **`agenterm-cc` 是独立 PE，进程内没有 Engine** —— 取语义方式见 §8.3.0
│
├── 5.7 产品面回调
│   ├── `product.cc.footer_line()` → 一行字符串（Phase 0 即可验证）
│   ├── `product.cc.nav_items()` / `empty_state(zone)` → 静态语义，可缓存（Phase 2）
│   ├── `product.cc.present(ctx)` → 逐帧行合成 —— **已移出 Phase 2，见 §8.3.3**
│   └── `product.*` catalog surface 注册
│
├── 5.8 更新通道
│   ├── Channel manifest（stable/beta）+ 签名
│   ├── 下载 → staging → drain UI → reload → 失败回滚
│   ├── 旧包/staging 的清理与配额（谁删、留几代）
│   └── audit event: `app_pack_update_applied`
│
├── 5.9 引擎默认化（QJS-Default）
│   ├── 根 `Cargo.toml` 现为 `default = []`；`script-qjs = ["agenterm-qjs"]`
│   ├── rquickjs 0.12.2 携带 QuickJS C 源码 → 六格矩阵每格都要能编 C
│   └── 这是把 QJS 内嵌进 Base 的真实前置，成本见 §7.2
│
└── 5.10 可观测性
    ├── pack 回调的 metric / event journal kind（§10.3 双调试栈的落点）
    ├── locale 从 native 传入 pack 的通道 + 缺失 i18n key 的回退
    └── 目前 `src/event_journal.rs` 的 EVENT_CATALOG 里没有 app-pack 相关 kind
```

---

## 6. 包形态：密封源码目录（树）

```text
agenterm.app 概念布局（解压后 = `<数据目录>/agenterm/app-pack/`）
├── manifest.json              # 密封目录的"身份证"
│   ├── schema: "agenterm.app-pack-manifest/v1"
│   ├── app_version: "0.3.1"
│   ├── engine: "qjs"          # 加载器按此选择 ScriptEngineBackend
│   ├── entry: "entry.js"
│   ├── requires_base: ">=0.1.18, <0.2.0"   # 双端窄区间，与 §10.2 对策一致
│   └── sha256: "abc..."
│
├── entry.js                   # 主入口：native ↔ script 的"插座"
│   // native 只调用这些具名 export function；内部实现可以 import 子模块
│   export function app_version()      { return "0.3.1" }
│   export function cc_footer_line()   { ... }
│   export function cc_nav_items()     { ... }
│   export function empty_state(zone)  { ... }
│   export function toolbar_actions()  { ... }
│
├── cc/                        # Control Center 模块
│   ├── nav.js                 # 导航状态机
│   ├── views.js               # 视图定义
│   ├── empty.js               # 空态文案（i18n key → 字符串）
│   └── layout.js              # Native-A 行合成（Phase 2 后期）
│
├── shell/                     # 主 GUI chrome 模块（Phase 4）
│   ├── toolbar.js             # toolbar action 顺序/可见性
│   ├── shortcuts.js           # 快捷键声明表
│   ├── context_menu.js        # 右键菜单项
│   └── welcome.js             # 欢迎页 copy
│
├── settings/                  # Settings 模块
│   ├── validators.js          # 用户输入校验规则
│   └── defaults.js            # 默认值
│
├── llm/                       # LLM 网关模块（可拆独立子包）
│   ├── routes.js              # 路由表
│   └── adapters/              # 站点适配器
│       ├── deepseek.js
│       └── openai.js
│
├── theme/                     # 主题应用逻辑
│   ├── tokens.js              # 从 skin JSON 提取 token → 应用到 CC 行
│   └── palette.js
│
└── lib/                       # 共享工具
    ├── fleet.js               # fleet.* 封装（已有：scripts/qjs/lib/fleet.js）
    └── product.js             # product.* 封装（native 回调的调用面）

（`pack.qjsc` 字节码缓存 **不进 v1** —— 与 §8.0 RA-2 一致。v1 的包里只有源码 +
manifest；字节码要不要落盘等 QJS 冷启动实测数字出来再定，见 §10.5。）

pack 目录**外**还有一个同级文件（不属于密封内容，用户改 pack 不会动到它）：
`app-pack.state.json` — { origin: factory|user|ota, installed_sha256, installed_at }

6.1 设计原则
│
├── 模块按产品域分目录（cc/ shell/ settings/ llm/ theme/）
│   ├── 每个目录是独立 ES module，通过 import 互引用
│   └── 目录树 = 产品模块树，一看就懂，不需要查映射表
│
├── entry.js 是 native ↔ script 的唯一接触面
│   ├── native 只调用 entry.js 里注册的具名 export function
│   ├── 内部实现可以 import 子模块、调用 fleet.*、读 settings JSON
│   └── native 不关心内部依赖图 → 脚本端可自由重构
│
├── lib/fleet.js 是已有资产（scripts/qjs/lib/fleet.js，209 行）
│   ├── 与 rh 的 fleet.* 同一语义、同一 `OPERATION_CATALOG`
│   │   └── 现为 **44 个操作**（`src/operations.rs:525` 起；`script_fleet.rs` 断言一一对应）
│   ├── CC 脚本可以 fleet.tab.close()、fleet.ui.snapshot()
│   └── 与 lua 的 scripts/lua/lib/fleet.lua（247 行）语义对齐，非逐行等长
│
├── 密封目录 = tar.zst → 发布为 .agp → 解压到 <数据目录>/agenterm/app-pack/
│   ├── 不是 .dll、不是 .wasm、不是字节码 blob —— 就是源码文件树
│   ├── 用户可以打开 app-pack/entry.js 读源代码
│   └── 用户可以 fork 一份改掉空态文案，丢回目录 → reload（开发模式）
│       └── 改动会让 origin 翻成 user（§3.3 ③），此后出厂包不再覆盖它
│
└── 第三方/用户可扩展
    ├── 官方 agenterm.app 是默认出厂包
    ├── 高级用户可以替换为社区 fork（`agenterm cli app-pack set-path <dir>`）
    └── 企业用户可以自建内部 channel（`agenterm cli app-pack set-channel <url>`）
```

---

## 7. 引擎策略：QJS 进 app pack，rh 留 Build/CI（树）

```text
7.1 结论：agenterm.app 最终用 QJS；rh 保留为构建/CI/一次性 task 引擎
│
├── 7.1.1 为什么 QJS 是 app pack 的正确引擎
│   ├── ① 跨平台 = 一份源码
│   │   ├── rh AOT 编译产物 .dll/.so —— Win/Linux/macOS 各一份
│   │   ├── QJS .js 纯文本 —— 一份跑所有平台
│   │   └── app pack 的目标是"一套脚本统一三端体验"，源码格式天然跨平台
│   │
│   ├── ② 热更新 / 开发体验
│   │   ├── rh: 改一行 → rustc 重编译 2–5s → 替换 .dll → reload
│   │   ├── qjs: 改一行 → 保存 → reload（解析 <200ms）
│   │   └── app pack 周级迭代 → "edit → reload → test" 秒级闭环
│   │
│   ├── ③ WebView 互通（CC Phase C 远期）
│   │   ├── rh: .dll 无法在浏览器里跑
│   │   ├── qjs: CC Phase C 的 WebView 壳可以直接 import 同一份 cc/nav.js
│   │   └── 零桥接：同一套模块在 native QJS 和 WebView 两个上下文里跑
│   │
│   ├── ④ 可审计性
│   │   ├── rh: .dll 是不透明二进制 —— 用户看不到 pack 做了什么
│   │   ├── qjs: .js 源码 —— 用户可以直接读 <数据目录>/agenterm/app-pack/entry.js
│   │   └── 开源随 repo 时，源码格式天然符合开源精神
│   │
│   ├── ⑤ 体积
│   │   ├── rh: .dll 含 rustc 生成的机器码 → 通常几百 KB 起步
│   │   └── qjs: 源码文本 → 20 行的 nav.js 就是 20 行文本
│   │
│   └── ⑥ 性能差在 app pack 场景里不相关
│       ├── CC 回调的实际负载：返回字符串、数组、对象（不是热路径）
│       ├── 60fps 渲染循环不经过脚本层（native 画像素）
│       ├── QJS 解析一个对象 < 1ms，比 native 画一帧快 3 个数量级
│       └── rh 的 AOT 优势在这里没有用武之地
│
├── 7.1.2 rh 去哪：保留且继续投入，但不在 app pack 路径上
│   ├── 构建 task / CI 脚本 ← 主场（永远 AOT）
│   │   └── scripts/rh/build.rh、check.rh、release.rh …
│   ├── 一次性自动化 / smoke / qualification
│   │   └── 用户写 .rh 脚本跑完即退出
│   ├── 需要原生性能的离线任务
│   │   └── 大规模文本处理、日志分析
│   └── agenterm-rh CLI 独立存在
│       └── `agenterm rh check/eval/run/pack …`（不做嵌入 + reload 那种长期驻留）
│
└── 7.1.3 双引擎共存（不是互砍）
    ├── 同一 catalog: fleet.* / std.* / product.* 两个引擎都能调
    ├── 同一 ScriptEngineBackend trait（script_engine.rs）
    ├── 不同场景:
    │   ├── agenterm.app（产品面）→ QJS engine
    │   └── scripts/rh/（构建/CI）→ rh engine (AOT)
    └── 不引入第三引擎到 app pack（lua 维持 CLI 地位）

7.2 选 QJS 的代价：它今天不在发布产物里
│
├── 7.2.1 现状
│   ├── 根 `Cargo.toml`：`default = []`，`script-qjs = ["agenterm-qjs"]` 是 opt-in
│   ├── `ScriptBackend::Qjs` 本身就是 `#[cfg(feature = "script-qjs")]`
│   └── 即"QJS pack 已对接"成立于 CLI，不成立于**默认构建的 Base**
│
├── 7.2.2 转默认要付什么
│   ├── rquickjs 0.12.2 携带 QuickJS **C 源码** → 六格矩阵每格都要能编 C
│   │   ├── aarch64-pc-windows-msvc 走 cargo-xwin（交叉 MSVC）
│   │   ├── aarch64-unknown-linux-gnu 走交叉链接
│   │   └── 这两格 v0.1.16 刚转绿，加 C 工具链依赖是**新增风险面**，不是既有风险
│   ├── 二进制体积 + 编译墙钟需重新测量（§2.1 的成本论证依赖这两个数）
│   └── 许可证清单需加 QuickJS（MIT）到 third-party notices
│
└── 7.2.3 结论：QJS-Default 是 Phase 0 的前置 gate，且排在最前
    ├── 它与 CC/pack 逻辑完全解耦 → 先做，失败也不浪费 Phase 0 的工作
    ├── 它顺带复验 v0.1.16 修好的两格交叉编译是否真稳
    └── 若某格编不过：按 `plan-v0.1.18.md` 的 Q0a/G2 停门，先在修工具链、
        独立 Runtime Component 或调整范围之间复核；不自动回退到目标相关的 rh AOT App，
        否则会失去“一包六格”的版本结果
```

---

## 8. 分期实施（树）

```text
8.0 前置：文档与对齐（无代码改动）
│
├── A0 本文定稿
│   ├── 纳入 `plan/agenterm-rhai-app.md` 的架构讨论作为 §12 交叉引用条目
│   ├── 与 `plan/design-rhai-rust-boundary.md` 三层边界对齐
│   └── 与 `plan/design-release-base-vs-apps.md` App Pack 条目对齐
│
└── A1 开放问题收口
    ├── RA-1: 首版 pack 只含 CC，不含主 GUI toolbar → 是
    ├── RA-2: pack 字节码缓存进 v1？→ 不进（rh AOT 天然 .dll；QJS 待冷启动实测）
    ├── RA-3: 远程 channel 自建 vs GitHub Release → 先 GitHub Release 资产
    ├── RA-4: manifest schema 名 → `agenterm.app-pack-manifest/v1`
    ├── RA-5: pack 源码开闭 → 随 repo 开源；内嵌出厂 pack 是 build artifact
    ├── RA-6: CC 独立 PE 怎么拿 pack 语义 → IPC + 缓存（§8.3.0），**Phase 0 就定死**
    └── RA-7: 签名密钥的保管/轮换/吊销 → Phase 3 前必须有答案（§10.4）
│
8.0.1 Gate 0 — QJS-Default（**排在 Phase 0 之前**，见 §7.2）
│
├── 把 `script-qjs` 转入根 `Cargo.toml` 的 `default`
├── 六格矩阵全绿（含 cargo-xwin 下的 aarch64-pc-windows-msvc 编 QuickJS C 源码）
├── 出数：二进制体积增量、冷编译墙钟增量、third-party notices 增补
└── 不过则停：Phase 0 不开工，按版本计划复核工具链或宿主形态；不自动回退 rh App
│
8.1 Phase 0 — 占位 pack + 自解包（最小可行链路）
│
├── 目标
│   ├── 验证：pack 可以被加载、可以被 reload、不杀 PTY
│   ├── 证据：`cc-snapshot` 多字段 `app_pack_version`
│   └── smoke：`scripts/qjs/app-pack-smoke.js` — 启动 → 检查 pack 版本 → reload → 再检查
│
├── 8.1.1 Native：自解包机制
│   ├── build.rs 增加：`include_bytes!("dist/agenterm-app.agp")` → 嵌入 PE 资源段
│   ├── 新模块 `src/app_pack.rs`
│   │   ├── `ensure_app_pack_extracted() → PathBuf`
│   │   ├── 路径经 `platform/policy/paths.rs::local_data_root_for_product_directory()`
│   │   │   └── 直接拼 `~/.local/share` 会撞 `src/platform/boundary_tests.rs` 的边界策略
│   │   ├── 读 `app-pack.state.json` → 三态判定（§3.3 ③）
│   │   ├── 需要解包 → 从嵌入字节解压 tar.zst
│   │   └── 写入数据目录 + 更新 state → 返回 pack 根路径
│   └── 新 CLI：`agenterm cli app-pack extract [--force]` / `factory-reset`
│
├── 8.1.2 Native：嵌入 Engine + loader
│   ├── 重构 `script_rh_pack.rs` → 支持非环境变量路径（当前只读 `AGENTERM_RH_PACK`）
│   ├── server 启动时：`AppPack::load_or_extract()` → `AppPackEngine`（OnceLock 长驻）
│   ├── `pack.version()` → 读 manifest.app_version（rh 形态时回落 manifest.rh_version）
│   └── CLI：`agenterm cli app-pack status` → 打印版本、路径、engine、origin
│
├── 8.1.3 Pack：占位 entry.js（QJS）
│   ├── 内容（~30 行，不调 fleet.*）：
│   │   // entry.js — Phase 0 占位
│   │   const APP_PACK_VERSION = "0.1.0";
│   │
│   │   export function app_version() {
│   │       return APP_PACK_VERSION;
│   │   }
│   │
│   │   export function cc_footer_line() {
│   │       return "agenterm.app/" + APP_PACK_VERSION;
│   │   }
│   ├── 构建：`agenterm qjs pack build --source entry.js --out dist/agenterm-app`
│   │   └── 产出：entry.js + manifest.json（v1 不落 pack.qjsc，见 RA-2）
│   └── 打包：tar + zstd → `dist/agenterm-app.agp`（build.rs 内嵌它）
│
├── 8.1.4 证据
│   ├── `agenterm cli ui-snapshot` 输出含 `app_pack_version: "0.1.0"`
│   ├── `agenterm cli app-pack status` → `version: 0.1.0, engine: qjs, origin: factory, path: <数据目录>/agenterm/app-pack/`
│   ├── smoke：启动 server → 检查 pack loaded → `app-pack reload` → 检查 pack reloaded
│   ├── smoke：无 pack 时 → 仍正常启动（Rust fallback，无 `app_pack_version` 字段）
│   └── smoke：出厂包版本更高 + origin=factory → 自动覆盖升级；origin=user → 不覆盖
│
├── 8.1.5 非目标
│   ├── 不做远程更新
│   ├── 不做 CC 内容生成
│   ├── 不做 on_frame 回调
│   └── 不做 pack 内 fleet.* 调用（占位包不碰 host API）
│
8.2 Phase 1 — 接一条竖线（验证 Strangler 模式）
│
├── 目标
│   ├── 验证：一条产品文案从 pack 来，Rust fallback 同内容
│   ├── 证据：pack 失败 → Rust 默认文案；pack 成功 → pack 文案
│   └── 建立迁移纪律：先数据后逻辑、双路径短存、每迁一块删 Rust 重复
│
├── 8.2.1 候选第一条竖线（选一条做）
│   ├── 方案 A：CC about/footer 文案
│   │   ├── pack: `fn cc_about_text() { "AgenTerm " + APP_PACK_VERSION + " · script-powered" }`
│   │   ├── native: `AppPack::cc_about_text().unwrap_or(DEFAULT_ABOUT_TEXT)`
│   │   └── 测试：切换 pack 版本 → about 文案变化
│   │
│   └── 方案 B：unavailable reason → user_message 映射表
│       ├── pack: `fn unavailable_reason(code) { REASONS[code] }`
│       ├── native: `AppPack::unavailable_reason(code).unwrap_or_else(|| hardcoded_reason(code))`
│       └── 测试：注入新 reason code → pack 返回文案 → native fallback
│
├── 8.2.2 Native：产品面回调注册
│   ├── `AppPackEngine` 增加 typed callback 方法
│   ├── 每个回调有：pack 函数名 + Rust fallback 闭包
│   └── 超时保护：**必须走引擎中断钩子，不能事后量墙钟**
│       ├── 事后判定"这次超了 50ms"时回调已经跑完/跑飞了，救不回来
│       ├── QJS：`rquickjs` 的 interrupt handler，按指令计数触发
│       ├── rh（若回退 rh 形态）：引擎 operation limit
│       └── 必须定义中断后引擎能否续用：默认**标记该 Engine 脏 → reload**，
│           因为脚本可能停在半个状态更新上
│
├── 8.2.3 迁移纪律（从此开始强制执行）
│   ├── 兜底按 I5 细则两段式（Phase 0–1 等价实现；Phase 2 起最小安全态）
│   ├── 双路径短存：同屏只允许一种 authority；迁移完成再删 Rust 分支
│   ├── 先数据后逻辑：先迁 JSON/copy/constants，再迁状态机
│   └── 每迁一块：删 Rust 重复 + 黑盒断言不变 + 同步改该项的 fallback 证据门
│
└── 8.2.4 非目标
    ├── 不做 CC 导航状态机迁移
    ├── 不做 on_frame 行生成
    └── 不做 toolbar 策略
│
8.3 Phase 2 — CC chrome 迁移（主战场）
│
├── 目标
│   ├── CC 的导航、空态、hyper_control 分区全部来自 pack
│   ├── native 只负责：hit-test、绘制、事件分发
│   └── 证据：CC snapshot 字段由 pack 驱动，native 只做像素
│
├── 8.3.0 前提：`agenterm-cc` 是独立 PE，Engine 在 server 进程 —— 怎么取语义
│   ├── ❌ CC 进程自建 Engine：违背 §10.6 单 Engine 纪律，内存翻倍，两份 pack 状态
│   ├── ✅ **走已有 IPC 取"静态语义"，CC 进程内缓存**
│   │   ├── CC 启动/reload 时拉一次：nav_items、empty_state、validators、app_version
│   │   ├── server 侧 pack reload → 推一次失效通知 → CC 重新拉
│   │   └── 拉取失败 → 用上一份缓存；无缓存 → I5 最小安全态
│   └── 这条必须在 Phase 0 就定死（RA-6），否则 Phase 2 推倒重来
│
├── 8.3.1 迁移顺序
│   ├── ① CC selected_view 默认值与 nav 标签（仍 native hit-test）
│   ├── ② hyper_control 空态分区 copy
│   ├── ③ Settings modal 文案与校验规则
│   └── ④ layout 行生成（大块，最后与 geometry 测试一起迁）
│
├── 8.3.2 新增 catalog surface（全部是**可缓存的静态语义**）
│   ├── `product.cc.selected_view(ctx) → view_id`
│   ├── `product.cc.nav_items() → [{id, label, enabled}]`
│   ├── `product.cc.empty_state(zone, locale) → {copy, action_label}`
│   └── 全部注册到 `product.*` 子空间（不塞进 `OPERATION_CATALOG`——
│       那是 fleet 操作目录，`script_fleet.rs` 有一一对应断言，混入会破坏它）
│
└── 8.3.3 非目标（本 Phase 明确剔除）
    ├── **`product.cc.present(ctx) → lines[]` 移出 Phase 2**
    │   ├── 逐帧把整屏行合成交给脚本 = 把渲染热路径拉进脚本层，与 I1 精神相悖
    │   ├── 每次刷新还要过一趟 CC↔server IPC（§8.3.0）
    │   └── 若将来仍要做：需先有帧预算实测 + 缓存失效模型，另立设计稿
    ├── 不做主 GUI toolbar/strip
    ├── 不做终端内右键菜单
    └── 不做 settings 存储逻辑（存储仍在 server 侧 Rust）
│
8.4 Phase 3 — 远程更新通道
│
├── 目标
│   ├── 用户可获取新 pack 而无需重装 Base
│   ├── 签名验证 + 用户确认 + 失败回滚
│   └── 证据：更新 smoke 覆盖下载→staging→apply→rollback 四条路径
│
├── 8.4.1 更新流程
│   ├── ① Base 启动 / 每 N 小时 / 用户点「检查更新」
│   ├── ② GET `https://agenterm.work/release/latest.json`
│   │   └── { app_pack_version, sha256, signature, release_notes, requires_base, channel }
│   ├── ③ 若本地旧 && requires_base 满足：
│   │   └── **静默下载** `.agp` 到 staging（后台，不弹窗、不阻塞）
│   ├── ④ 下载完成 + 校验 sha256 + 签名通过：
│   │   └── **非侵入提示**（CC footer / 气泡）：「新版本就绪，是否立即加载？」
│   ├── ⑤ 用户确认 → `app-pack apply --staging` → drain UI → reload Engine
│   │   ├── 用户拒绝 → staging 保留（下次不再重复下载），下次启动再问
│   │   └── 失败 → `app-pack rollback` → 恢复旧 pack
│   ├── ⑥ staging 目录：`<数据目录>/agenterm/app-pack-staging/`
│   ├── ⑦ apply 成功 → origin 置 ota + 记录新 sha256
│   └── ⑧ audit: `app_pack_update_downloaded` / `app_pack_update_applied` / `app_pack_update_failed`
│
├── 8.4.2 Native 新增
│   ├── `update.check(channel)` → manifest
│   ├── `update.download(manifest) → staging_path`
│   ├── `update.verify(staging_path) → bool`
│   ├── `update.apply(staging_path)` → 原子替换 + reload
│   └── `update.rollback()` → 恢复旧 pack
│
├── 8.4.3 磁盘生命周期（谁删、留几代）—— 不写清楚就会无限堆积
│   ├── 旧 pack：只保留**一代**（`app-pack.prev/`），供 rollback；再更新即覆盖
│   ├── staging：apply 成功即删；用户拒绝则保留（避免重复下载），
│   │   但下一次 channel 版本变了就替换掉，且设**单份上限**
│   ├── 每次启动做一次孤儿清扫（崩溃留下的半成品 staging）
│   └── `app-pack doctor` 报告三者占用
│
└── 8.4.4 非目标
    ├── 不做静默 overnight 替换
    ├── 不做无签名的 URL
    ├── 不做 pack 内自更新 bootstrap
    └── 不做增量/差分更新（全量 .agp 替换）
│
8.5 Phase 4 — 主 GUI chrome（远期，最后做）
│
├── 目标
│   ├── toolbar 行为策略、快捷键映射、右键菜单 → pack
│   ├── 仍 native 渲染（按钮、菜单、tooltip 像素），pack 只决定"显示什么"
│   └── 证据：同一 pack 在 Win/Unix 产生相同的 toolbar 语义
│
├── 8.5.1 候选迁移项
│   ├── toolbar action 映射（哪些 action 可见、顺序）
│   ├── 快捷键表（pack 声明，native 注册）
│   ├── 右键菜单项与顺序
│   ├── 空态欢迎页 copy
│   └── tab editor 校验规则与提示文案
│
└── 8.5.2 非目标
    ├── 不做终端网格内渲染
    ├── 不做字体/颜色渲染管线
    └── 不做窗口管理（最小化/最大化/DPI 策略）
│
8.6 QJS 前置工作 + rh 辅轨（独立于 Phase 时间线）
│
├── QJS-Default：把 `script-qjs` 转为默认 feature（**Gate 0，见 §7.2 / §8.0.1**）
│   └── 排在 QJS-M6 之前：surface 校验做得再全，编不出来也上不了船
│
├── QJS-M6：App Host ABI literal operation 静态校验（Phase 0 前置条件）
│   ├── 以版本化 App Host ABI catalog 与共享 `OPERATION_CATALOG` 中实际暴露给 App 的子集为权威
│   ├── Rh 的 `SHIPPED_SURFACE_PATHS` 只作历史对账输入，不是要求 App 暴露的数量目标
│   ├── 已知 literal 在 qjs `check`/`check-many` 通过；未知 literal typed fail-closed；
│   │   动态表达式标为不可静态证明，不虚报通过
│   └── 这是 QJS 作为 product pack 引擎的出厂资格；`capability` 仍仅表示发现/兼容元数据
│
├── QJS-Embed：嵌入模式接入（Phase 0 同步做）
│   ├── `AppPackEngine` 按 manifest.engine 选择 ScriptEngineBackend
│   ├── `QjsPack::eval(&host)` 目前是 run-to-exit → 需长驻 Runtime/Context 化
│   ├── entry.js export function → native 回调解析
│   ├── interrupt handler 接入（§8.2.2 的超时保护靠它）
│   └── smoke：同内容 `entry.js` → CC footer_line 通过
│
├── QJS-Module：ES module import 跨目录引用
│   ├── `cc/nav.js` 可以 `import { fleet } from "../lib/fleet.js"`
│   ├── `ProjectModuleResolver`（agenterm-qjs/src/module_resolver.rs）已有 root 解析
│   └── 验证：多模块 pack → 全部 resolve → eval 成功（含 `..` 上跳不越出 pack 根）
│
├── QJS-WebView：远期 CC Phase C WebView 互通（Phase 3+）
│   ├── pack 内 JS 子模块可在 WebView 壳里直接 eval
│   ├── 与 native QJS 共享同一 fleet.* / product.* 语义
│   └── 不在本计划 Phase 0–4 范围内
│
└── rh 辅轨：保持健康，不进 app pack
    ├── rh AOT 继续投入（不因 QJS 胜出而砍）
    ├── 场景：scripts/rh/build.rh、check.rh、release.rh、smoke.rh …
    ├── agenterm-rh CLI 保持：`agenterm rh check/eval/run/pack …`
    └── 不参与嵌入 + reload 那种长期驻留路径
```

---

## 9. 决策表

| 问题 | 选项 | 决定 | 理由 |
|------|------|------|------|
| v1 引擎 | rh 单轨 / QJS 单轨 / 双轨 | **QJS 进 app pack，rh 留 Build/CI**；Gate 0 失败则停门复核宿主形态 | QJS 跨平台源码一份、hot reload 秒级、CC Phase C WebView 互通；目标相关的 rh AOT 不满足“一包六格” |
| QJS 默认化时机 | Phase 0 内做 / Phase 0 之前 / 留到 Phase 2 | **Phase 0 之前，Gate 0** | 与产品逻辑解耦；失败可零损失回退引擎选型（§7.2） |
| CC 独立 PE 取 pack 语义 | CC 自建 Engine / IPC 拉静态语义 / 逐帧 IPC | **IPC 拉静态语义 + CC 侧缓存** | 自建 Engine 破单 Engine 纪律；逐帧 IPC 把渲染拉进脚本层（§8.3.0） |
| 迁移后的 fallback 语义 | 永远等价 Rust 实现 / 两段式 | **两段式**（Phase 0–1 等价，Phase 2 起最小安全态） | "等价"与"迁完删 Rust"逻辑互斥（I5 细则） |
| 回调超时保护 | 事后量墙钟 / 引擎中断钩子 | **引擎中断钩子** | 事后判定时回调已跑飞，救不回来（§8.2.2） |
| Pack 格式 | .dll / .js 源码 / 字节码 blob | **密封源码目录（tar.zst）** | 用户可读可改可 fork；跨平台一份；开发 edit→reload 秒级 |
| Pack 结构 | 单文件 / 多模块目录 | **按产品域分目录（cc/shell/settings/llm/theme/lib）** | 目录树 = 产品模块树；entry.js 是 native↔script 唯一接触面 |
| Pack 粒度 | 单 `agenterm.app` / 多独立 pack | **单 monorepo pack + 模块** | LLM 可拆子模块；避免多 pack 版本矩阵爆炸 |
| Engine 进程模型 | 内嵌 / `agenterm-rh` 子进程 | **内嵌** product engine | CLI 仍用独立 PE；pack 内嵌避免 IPC 延迟 |
| 远程更新默认 | 开 / 关 / 检查+静默下载+提示 | **检查 + 静默下载 + 提示加载** | 下载不打断用户；下载完成后非侵入提示；用户决定何时 reload |
| Pack 源码 | 开源随 repo / 闭源 channel | **随 repo 开源** | 内嵌出厂 pack 是 build artifact |
| 出厂包升级策略 | 存在即跳过 / 版本比对三态 | **三态（factory/user/ota）** | 否则出厂包永远升不上去，或静默覆盖用户 fork（§3.3 ③） |
| 旧包保留代数 | 不留 / 一代 / 多代 | **一代 `app-pack.prev/`** | 够 rollback，不会无限堆积（§8.4.3） |
| CC 原生 PE | 保留 / 合并 | **保留** `agenterm-cc` thin PE | pack 内容驱动；PE 壳不变 |
| UX 统一目标 | 像素 / 语义 | **语义 + layout 契约** | 像素由 native/theme 保证 |
| 自解包格式 | tar+zstd / zip / 平面目录复制 | **tar+zstd** 密封归档 | 跨平台一致；`include_bytes!` 进 PE 资源段 |
| QJS 引擎在 pack 内的角色 | 替代 rh / 并行共存 / 远期选项 | **并行共存** | manifest.engine 选择；同一 catalog 共享 |
| 第一个 pack 内容范围 | 仅 CC / CC + LLM / 全产品 | **仅 CC** | CC 是独立 PE + composed lines，最适合 Strangler |

---

## 10. 风险目录（树）

```text
风险
├── 10.1 范围蠕变 🔴
│   ├── 症状：Phase 0 就想做 on_frame / toolbar / 远程更新
│   ├── 后果：自解包都没跑通就开始塞产品逻辑 → 全线崩塌
│   └── 对策：Phase 0 占位 pack ≤ 30 行 JS；Phase 1 只接一条竖线；gate 写死
│
├── 10.2 Host API 版本耦合 🟠
│   ├── 症状：pack 调 `fleet.tab.close`，Base 改了参数形状 → pack 静默行为错误
│   ├── 后果：更新 pack 后终端行为异常，用户不知是 pack 还是 Base 的问题
│   └── 对策：manifest.requires_base **双端窄区间**（`>=x, <y`）；`app-pack doctor`
│       兼容性检查；CI 采样矩阵
│
├── 10.3 双调试栈 🟠
│   ├── 症状：bug 在 pack 脚本还是 native loader？栈追踪断在 FFI 边界
│   ├── 后果：排查时间翻倍；用户报告"CC 空态不对"无法定位
│   └── 对策：pack 行号映射 + 结构化 panic + `app-pack doctor` +
│       **event journal 新增 app-pack kind**（否则线上问题什么痕迹都不留，见 §5.10）
│
├── 10.4 pack 权限 = 用户权限，签名是唯一边界 🔴
│   ├── 症状：I6 说 pack 不做 sandbox，而 pack 能调全量 fleet.*（含 destructive）
│   │   └── 所以"签名通过的 pack" ≡ 用户权限下的任意代码执行（I11）
│   ├── 症状：channel 被劫持 → 恶意 pack 下发 → 用户无感知
│   ├── 后果：远程代码执行；供应链攻击面
│   └── 对策：
│       ├── Publisher 密钥签名 + sha256 比对 + 用户显式确认 + 离线可拒绝
│       ├── **密钥保管/轮换/吊销方案**（RA-7，Phase 3 前必须有答案）
│       ├── **离线校验根**：公钥随 Base 编译进二进制，不从网络取
│       └── **factory-reset 通道**：任何时候可丢弃 ota/user 包回到出厂内嵌包
│
├── 10.5 启动延迟 🟡
│   ├── 症状：首次启动解包 + 引擎初始化 → 冷启动增加
│   ├── 后果：用户感知"变慢了"
│   └── 对策：QJS 无 AOT 编译，主要成本是解包 + parse；
│       Gate 0 顺带量一次冷启动增量，超阈值才考虑 pack.qjsc 字节码缓存（RA-2）
│
├── 10.6 Engine 内存 🟡
│   ├── 症状：多窗口 + 每窗一个 Engine → 内存线性增长
│   ├── 后果：10 窗 = 10× Engine 内存
│   └── 对策：单进程单 Engine + reload 纪律；pack 不持有大对象；
│       CC 独立 PE 不自建 Engine（§8.3.0）
│
├── 10.7 两套 truth 🟠
│   ├── 症状：pack 缓存了 Fleet 状态 → server 侧变了 pack 不知道
│   ├── 后果：CC 显示过期的 tab 列表
│   └── 对策：pack 只投影 server snapshot；每次回调 native 传入最新 ctx
│       └── 注意 §8.3.0 的 CC 侧缓存只缓存**静态语义**（nav/文案/校验规则），
│           不缓存 Fleet 状态 —— 两者别混
│
├── 10.8 pack 反复崩溃 🟠
│   ├── 症状：pack 每次加载/每次回调都 panic → 每帧都走崩溃路径
│   ├── 后果：I5 的"每次调用兜底"变成"每次调用都先崩一次"，日志刷屏、CPU 空转
│   └── 对策（I12）：连续 N 次失败 → 该 pack 标记 disabled + 落 state →
│       进最小安全态 + 一行可见提示 + `app-pack doctor` 给出原因和 factory-reset 建议
│
├── 10.9 磁盘占用失控 🟡
│   ├── 症状：pack + prev + staging + 崩溃残留反复堆积
│   ├── 后果：用户数据目录无声增长
│   └── 对策：§8.4.3 的保留代数与启动期孤儿清扫
│
└── 10.10 i18n 断层 🟡
    ├── 症状：`cc/empty.js` 按 i18n key 出文案，但 locale 没有从 native 传进去
    ├── 后果：pack 只能出一种语言，或各模块各猜一套
    └── 对策：locale 作为回调 ctx 的必填字段；缺失 key 的回退规则写进 pack 契约
```

---

## 11. 证据门（每 Phase 出证）

| Phase | 证据类型 | 具体门 |
|-------|---------|--------|
| **Gate 0** | matrix | `script-qjs` 默认开启后，六格矩阵全绿（含 cargo-xwin 下编 QuickJS C 源码） |
| **Gate 0** | 体积/时间 | 记录二进制体积增量与冷编译墙钟增量；third-party notices 含 QuickJS |
| QJS-M6 | CI | qjs `check` 对齐版本化 App Host ABI/实际公开 operation 子集；未知 literal typed fail-closed，不继承 Rh 全量 surface 数量 |
| Phase 0 | smoke | `scripts/qjs/app-pack-smoke.js`：启动→pack loaded→reload→pack reloaded |
| Phase 0 | snapshot | `ui-snapshot` 含 `app_pack_version` 字段 |
| Phase 0 | CLI | `app-pack status` 打印 version / engine: qjs / origin / path（路径由 policy 解析，非硬编码） |
| Phase 0 | fallback | 无 pack 时 server 正常启动（无 `app_pack_version` 字段） |
| Phase 0 | 三态 | factory 包版本更高 → 自动升级；origin=user → 不覆盖；`factory-reset` 可回出厂 |
| Phase 0 | entry.js | `entry.js` export function 全部能被 native 正确调用并拿到返回值 |
| Phase 0 | 边界 | `boundary_tests.rs` 通过：app_pack 模块不含平台 cfg / 硬编码路径 |
| Phase 1 | callback | pack 切换版本 → CC about 文案变化；pack 删除 → Rust 等价实现同内容 |
| Phase 1 | 中断 | 死循环脚本被 interrupt handler 打断 → 走 fallback + Engine 标脏重载（不是墙钟事后判定） |
| Phase 1 | 可观测 | 每次 fallback 落一条 event journal 记录（kind + 回调名 + 原因） |
| Phase 1 | 熔断 | 连续 N 次失败 → pack disabled + 可见提示 + doctor 给出原因 |
| Phase 2 | snapshot | CC snapshot 字段由 pack 驱动；native 只做 hit-test |
| Phase 2 | IPC | CC 独立 PE 经 IPC 取静态语义并缓存；server 端 reload → CC 收到失效并重拉 |
| Phase 2 | fallback | **改断言口径**：pack 失效时是"降级态可用且可诊断"，不再断言内容等价（I5 细则） |
| Phase 2 | parity | 同一 pack 在 Win/Unix 产生相同 CC 语义（`cc-snapshot` diff） |
| Phase 2 | i18n | 切 locale → 文案跟随；缺失 key → 按回退规则出可读文案，不出 key 原文 |
| Phase 3 | update smoke | 静默下载→校验→提示→apply 成功；apply 失败→rollback 恢复 |
| Phase 3 | signature | 篡改 pack → verify 失败 → 拒绝 apply；无签名 pack → 拒绝；公钥来自二进制非网络 |
| Phase 3 | 磁盘 | 反复更新 10 次后：pack + prev + staging 占用有上界；孤儿 staging 被清扫 |
| Phase 3 | endpoint | `https://agenterm.work/release/latest.json` 可达且 schema 兼容 |
| Phase 4 | toolbar | 同一 pack 在 Win/Unix 产生相同 toolbar 语义 |

---

## 12. 交叉引用

| 文档 | 关系 |
|------|------|
| `plan/agenterm-rhai-app.md` | 架构讨论稿 rev1；本文是它的执行投影 |
| `plan/design-rhai-rust-boundary.md` | L1/L2/L3 三层边界 SSOT |
| `plan/design-release-base-vs-apps.md` | Base vs Apps 分轨发布设计 |
| `plan/design-scripting-boundary-comparison.md` | Rhai/Lua/QJS 引擎边界对照 |
| `plan/design-script-engine-trait.md` | `ScriptEngineBackend` trait 设计 |
| `plan/ARCHITECTURE.md` | 现行结构 SSOT；三层边界 |
| `plan/archive/plan-v0.1.17.md` | v0.1.17 收口版；本计划在其后执行 |
| `plan/plan-v0.1.18.md` | v0.1.18 版本执行投影；拥有该版本的范围、Gate 与验收口径，本文继续拥有 App Pack 架构和 Phase 细节 |
| `prd/PRD_02_10_rhai_scripting.md` | Script 引擎家族产品归属 |
| `prd/PRD_02_02_executable_family.md` | 可执行文件家族 |
| `plan/design-llm-gateway-rhai-logic-pack.md` | LLM Logic Pack（与 CC pack 可并行） |
| `plan/design-cc-hyper-control-agent.md` | CC 超控设计 |
| `docs/agenterm-rh-runtime.md` | Script Runtime 用户文档（rev1 写作 `agenterm-rhai-runtime.md`，已随引擎改名） |
| `src/platform/policy/paths.rs` | 数据目录解析 SSOT（`local_data_root_for_product_directory`） |
| `src/platform/boundary_tests.rs` | 平台边界策略，约束 app_pack 模块不得自带 cfg/硬编码路径 |
| `src/operations.rs` | `OPERATION_CATALOG`（44 个操作）；`product.*` 不并入此表 |
| `crates/agenterm-rh/src/shipped_surfaces.rs` | Rh 历史 surface 盘点输入；不是 App Host ABI 或 QJS-M6 的数量目标 |

---

## 13. 版本列车对齐

```text
v0.1.16（进行中，与本计划无关）
└── 发布战役：ISA×2 / OS×3 全绿 → candidate → release（见 plan/ci-green-handoff.md）
    └── 本计划的 Gate 0 依赖它修好的两格交叉编译，故排在其后

v0.1.17（收口版）
├── 主题：发布链证据 + 安装尾 + 脚本引擎深化 + 低成本卫生
├── 本计划占用：0（仅文档对齐）
└── 为 v0.1.18 做准备：A0 定稿 + A1 开放问题收口（含 RA-6 CC 取语义方式、RA-7 密钥方案）

v0.1.18（Portable App Substrate）
├── 版本范围、依赖树与验收：见 plan/plan-v0.1.18.md（版本执行 SSOT）
├── 本文拥有的实现细节：Gate 0 + Phase 0；实际开工仍服从版本计划逐 Gate 收敛
├── 决定性证据：同一 `.agp` SHA 被六格消费，App-only lane 不调用 Cargo/不重编 Base
└── QJS 采用门不过则停止 Phase 0 并重新选择宿主形态
    └── 不自动回退到目标相关的 Rh AOT App，也不虚报“一包六格”

v0.1.19+（按 Phase 推进）
├── Phase 1：接一条竖线（QJS callback → CC footer）+ 中断保护 + 熔断 + 可观测
├── Phase 2：CC chrome 迁移（静态语义，不含逐帧 present）+ fallback 口径切换
├── Phase 3：远程更新通道（agenterm.work/release/latest.json）+ 磁盘生命周期
├── Phase 4：主 GUI chrome
└── rh 辅轨：Build/CI 持续投入，不进 app pack 路径
```
