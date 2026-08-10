# agenterm.app：脚本应用包独立计划书

| 字段 | 值 |
|------|-----|
| **文档** | 脚本应用包的完整分期计划——自解包、双引擎策略、Strangler 渐进迁移 |
| **日期** | 2026-08-10 |
| **状态** | 定稿，待 v0.1.17 收口后授权开工 |
| **前置** | `plan/agenterm-rhai-app.md`（架构讨论稿 rev1）、`plan/design-rhai-rust-boundary.md`（边界 SSOT）、`plan/design-release-base-vs-apps.md`（发布分轨）、`plan/design-scripting-boundary-comparison.md`（引擎边界对照）、`plan/ARCHITECTURE.md`（结构 SSOT） |
| **产品归属** | `prd/PRD_02_10_rhai_scripting.md` §Layered deployment、`prd/PRD_02_02_executable_family.md` |
| **决策人** | 产品范围决策；引擎选型在 §7 决策表 |

---

## 1. 一句话

**把 agenterm 的产品行为从 Rust 源码逐步迁移到密封的脚本应用包（`agenterm.app`），**
**运行时自解包、可独立更新；PTY/渲染/Server 内核永久 native。**
**CI 成本：Base 全矩阵 6 格编译保留给内核变更；应用层更新 = 上传一个 .agp 文件。**

---

## 2. 问题与动机（树）

```text
为什么做（根）
├── 2.1 Base 发布太贵、太慢
│   ├── Candidate 需全平台 stress qualification（Windows 真实 ConPTY + 全矩阵 6 格）
│   ├── 改一句空态文案 ≈ 重跑整个 release gate
│   └── 用户为一个 CC 文案修复等 0.1.x → 0.1.y 整版
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
├── 2.4 Base 发布的 CI 成本过高
│   ├── Release Candidate 需全矩阵 6 格编译：`{x86_64, aarch64} × {win, lnx, osx}`
│   ├── Windows stress qualification 需真实 ConPTY（Wine 无法模拟）
│   ├── 改一句空态文案 → 重跑全平台 gate → 等待 30–60 分钟 CI 墙钟
│   ├── Base 每发一版 = 6 个平台 zip + macOS 签名/公证 + Windows 签名
│   └── agenterm.app 更新 = 上传一个 .agp 文件 → CI 零成本（只是静态文件）
│
└── 2.5 已有成熟基础无需从零建设
    ├── Rhai AOT pack 已是生产级（`agenterm-rh/pack.rs`：build→load→entry→cc_lines）
    ├── QJS pack 已对接（`agenterm-qjs/pack.rs`：load→verify→eval_entry_with_host）
    ├── 跨引擎共享层已就绪（`agenterm-script-common/pack_support.rs`）
    └── in-process loader 有先例（`script_rh_pack.rs`：cached_rh_pack + try_load_rh_pack_from_env）
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
│   ├── 路径：`~/.local/share/agenterm/app-pack/`（解压后运行副本）
│   ├── 格式：密封目录，含 `manifest.json` + entry 脚本 + 子模块
│   ├── 内容（渐进）：CC 导航 → 空态文案 → hyper_control 分区 → LLM 路由 → toolbar 策略
│   ├── 更新：独立 channel，签名 + 显式用户确认 + 回滚
│   └── 来源：出厂内嵌 → 首次启动解压 → 后续远程更新覆盖
│
└── 3.3 启动流程
    ├── ① agenterm.exe 启动
    ├── ② 检查 `~/.local/share/agenterm/app-pack/manifest.json`
    ├── ③ 不存在 → 从 PE 资源段解出内嵌出厂归档 → 写入用户目录
    ├── ④ 加载 manifest + entry → 长驻 Engine（One Engine Per Process）
    ├── ⑤ Native 通过 catalog Facade 调用 pack 获取产品语义
    └── ⑥ （可选）后台嗅探远程更新 → 用户确认 → 下载 → staging → reload

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
    │           ~/.local/share/   用于：Engine 加载、用户检视/修改   │
    │           agenterm/app-pack/                                 │
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
    ├── 引用路径时：写 `~/.local/share/agenterm/app-pack/`
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
| I5 | **pack 失败 → Rust fallback（永远有兜底）** | 用户无感；同屏只允许一种 authority |
| I6 | **pack 不做 permission sandbox** | 审批/配额仍在 Agent harness/native |
| I7 | **pack 不做 npm 依赖树** | 整包替换密封目录；不传递求解 |
| I8 | **不新增第二套 Rhai runtime / host API** | 复用现有 `fleet.*` / `std.*` / catalog；增量 `product.*` |
| I9 | **不静默远程替换** | 签名 + 显式用户确认 + 回滚 |
| I10 | **不新增独立 PE** | pack 是数据，不是进程；Engine 内嵌 |

---

## 5. 已有基础设施（盘点）

```text
✅ 可直接复用
├── 5.1 Rhai Pack（agenterm-rh）
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
│   ├── pack.rs: QjsPack::load(dir) → manifest + source
│   ├── pack.rs: build_pack_dir(source, dir) → bytecode hash + manifest
│   ├── manifest.rs: QjsPackManifest { schema, qjs_version, bytecode_hash, source_hash, entry_file }
│   ├── compile.rs: compile_qjs(source) → bytecode + hash
│   ├── gen_module: `scripts/qjs/lib/fleet.js`（与 rh fleet 语义对齐）
│   └── 当前用法：CLI `agenterm qjs pack` / `check` / `eval`
│
├── 5.3 跨引擎共享层（agenterm-script-common）
│   ├── crate: `crates/agenterm-script-common/`
│   ├── pack_support.rs: verify_file_hash / hash_source（lua + qjs 共用）
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
│   ├── pack.version() / pack.reload() → catalog 新 surface
│   └── CLI: `agenterm cli app-pack status|reload|doctor`
│
├── 5.6 嵌入模式
│   ├── 长驻 Engine（不是跑完即退出的 task）
│   ├── Engine init 在 server 进程启动时，reload 不杀 PTY
│   └── 多窗口共享一个 Engine（不是每窗一个）
│
├── 5.7 产品面回调
│   ├── `product.cc.footer_line()` → 一行字符串（Phase 0 即可验证）
│   ├── `product.cc.present(ctx)` → CC Native-A 行合成（Phase 2+）
│   └── `product.*` catalog surface 注册
│
└── 5.8 更新通道
    ├── Channel manifest（stable/beta）+ 签名
    ├── 下载 → staging → drain UI → reload → 失败回滚
    └── audit event: `app_pack_update_applied`
```

---

## 6. 包形态：密封源码目录（树）

```text
agenterm.app 概念布局（解压后 = `~/.local/share/agenterm/app-pack/`）
├── manifest.json              # 密封目录的"身份证"
│   ├── schema: "agenterm.app-pack-manifest/v1"
│   ├── app_version: "0.3.1"
│   ├── engine: "qjs"          # 加载器按此选择 ScriptEngineBackend
│   ├── entry: "entry.js"
│   ├── requires_base: ">=0.1.18"
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
├── lib/                       # 共享工具
│   ├── fleet.js               # fleet.* 封装（已有：scripts/qjs/lib/fleet.js）
│   └── product.js             # product.* 封装（native 回调的调用面）
│
└── pack.qjsc                  # 字节码 + manifest 内 sha256 校验（可选缓存）
    manifest.json              # （同一份，pack 根目录）

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
├── lib/fleet.js 是已有资产（scripts/qjs/lib/fleet.js）
│   ├── 与 rh 的 fleet.* 同一语义、同一 OPERATION_CATALOG（77 个操作）
│   ├── CC 脚本可以 fleet.tab.close()、fleet.ui.snapshot()
│   └── 与 lua 的 scripts/lua/lib/fleet.lua 近行对行一致
│
├── 密封目录 = tar.zst → 发布为 .agp → 解压到 ~/.local/share/agenterm/app-pack/
│   ├── 不是 .dll、不是 .wasm、不是字节码 blob —— 就是源码文件树
│   ├── 用户可以打开 ~/.local/share/agenterm/app-pack/entry.js 读源代码
│   └── 用户可以 fork 一份改掉空态文案，丢回目录 → reload（开发模式）
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
│   │   ├── qjs: .js 源码 —— 用户可以直接读 ~/.local/share/agenterm/app-pack/entry.js
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
```

---

---

## 7. 分期实施（树）

```text
7.0 前置：文档与对齐（无代码改动）
│
├── A0 本文定稿
│   ├── 纳入 `plan/agenterm-rhai-app.md` 的架构讨论作为 §9 引用
│   ├── 与 `plan/design-rhai-rust-boundary.md` 三层边界对齐
│   └── 与 `plan/design-release-base-vs-apps.md` App Pack 条目对齐
│
└── A1 开放问题收口
    ├── RA-1: 首版 pack 只含 CC，不含主 GUI toolbar → 是
    ├── RA-2: pack 字节码缓存进 v1？→ rh AOT 天然 .dll，QJS 暂不进
    ├── RA-3: 远程 channel 自建 vs GitHub Release → 先 GitHub Release 资产
    ├── RA-4: manifest schema 名 → `agenterm.app-pack-manifest/v1`
    └── RA-5: pack 源码开闭 → 随 repo 开源；内嵌出厂 pack 是 build artifact
│
7.1 Phase 0 — 占位 pack + 自解包（最小可行链路）
│
├── 目标
│   ├── 验证：pack 可以被加载、可以被 reload、不杀 PTY
│   ├── 证据：`cc-snapshot` 多字段 `app_pack_version`
│   └── smoke：`scripts/rh/app-pack-smoke.rh` — 启动 → 检查 pack 版本 → reload → 再检查
│
├── 7.1.1 Native：自解包机制
│   ├── build.rs 增加：`include_bytes!("dist/agenterm-app.agp")` → 嵌入 PE 资源段
│   ├── 新模块 `src/app_pack.rs`（或 `src/platform/policy/app_pack.rs`）
│   │   ├── `ensure_app_pack_extracted() → PathBuf`
│   │   ├── 检查 `~/.local/share/agenterm/app-pack/manifest.json`
│   │   ├── 不存在 → 从嵌入字节解压（tar/zstd 或平面目录）
│   │   └── 写入用户目录 → 返回 pack 根路径
│   └── 新 CLI：`agenterm cli app-pack extract [--force]`
│
├── 7.1.2 Native：嵌入 Engine + loader
│   ├── 重构 `script_rh_pack.rs` → 支持非环境变量路径（当前只读 `AGENTERM_RH_PACK`）
│   ├── server 启动时：`AppPack::load_or_extract()` → `AppPackEngine`（OnceLock 长驻）
│   ├── `pack.version()` → 读 manifest.rh_version / manifest.app_version
│   └── CLI：`agenterm cli app-pack status` → 打印版本、路径、engine
│
├── 7.1.3 Pack：占位 entry.js（QJS）
│   ├── 内容（~30 行）：
│   │   ```js
│   │   // entry.js — Phase 0 占位
│   │   const APP_PACK_VERSION = "0.1.0";
│   │   const ENGINE = "qjs";
│   │
│   │   export function app_version() {
│   │       return APP_PACK_VERSION;
│   │   }
│   │
│   │   export function cc_footer_line() {
│   │       return `agenterm.app/${APP_PACK_VERSION}`;
│   │   }
│   │   ```
│   ├── 构建：`agenterm qjs pack build --source entry.js --out dist/agenterm-app`
│   │   └── 产出：entry.js + pack.qjsc + manifest.json
│   └── 打包：`tar -cf dist/agenterm-app.agp -C dist/agenterm-app .`
│
├── 7.1.4 证据
│   ├── `agenterm cli ui-snapshot` 输出含 `app_pack_version: "0.1.0"`
│   ├── `agenterm cli app-pack status` → `version: 0.1.0, engine: rh, path: ~/.local/share/agenterm/app-pack/`
│   ├── smoke：启动 server → 检查 pack loaded → `app-pack reload` → 检查 pack reloaded
│   └── smoke：无 pack 时 → 仍正常启动（Rust fallback，无 `app_pack_version` 字段）
│
├── 7.1.5 非目标
│   ├── 不做远程更新
│   ├── 不做 CC 内容生成
│   ├── 不做 on_frame 回调
│   └── 不做 QJS engine 切换
│
7.2 Phase 1 — 接一条竖线（验证 Strangler 模式）
│
├── 目标
│   ├── 验证：一条产品文案从 pack 来，Rust fallback 同内容
│   ├── 证据：pack 失败 → Rust 默认文案；pack 成功 → pack 文案
│   └── 建立迁移纪律：先数据后逻辑、双路径短存、每迁一块删 Rust 重复
│
├── 7.2.1 候选第一条竖线（选一条做）
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
├── 7.2.2 Native：产品面回调注册
│   ├── `AppPackEngine` 增加 typed callback 方法
│   ├── 每个回调有：pack 函数名 + Rust fallback 闭包
│   └── 超时保护：单次回调 > 50ms → 走 fallback + 记 metric
│
├── 7.2.3 迁移纪律（从此开始强制执行）
│   ├── Rust fallback 永远存在（pack 编译失败 / panic / 超时 → Rust 行为）
│   ├── 双路径短存：同屏只允许一种 authority；迁移完成再删 Rust 分支
│   ├── 先数据后逻辑：先迁 JSON/copy/constants，再迁状态机
│   └── 每迁一块：删 Rust 重复 + 黑盒断言不变
│
└── 7.2.4 非目标
    ├── 不做 CC 导航状态机迁移
    ├── 不做 on_frame 行生成
    └── 不做 toolbar 策略
│
7.3 Phase 2 — CC chrome 迁移（主战场）
│
├── 目标
│   ├── CC 的导航、空态、hyper_control 分区全部来自 pack
│   ├── native 只负责：hit-test、绘制、事件分发
│   └── 证据：CC snapshot 字段由 pack 驱动，native 只做像素
│
├── 7.3.1 迁移顺序
│   ├── ① CC selected_view 默认值与 nav 标签（仍 native hit-test）
│   ├── ② hyper_control 空态分区 copy
│   ├── ③ Settings modal 文案与校验规则
│   ├── ④ CC Native-A 行合成（`product.cc.present(ctx) → lines[]`）
│   └── ⑤ layout 行生成（大块，最后与 geometry 测试一起迁）
│
├── 7.3.2 新增 catalog surface
│   ├── `product.cc.selected_view(ctx) → view_id`
│   ├── `product.cc.nav_items() → [{id, label, enabled}]`
│   ├── `product.cc.empty_state(zone) → {copy, action_label}`
│   ├── `product.cc.present(ctx) → lines[]`（Phase 2 后期）
│   └── 全部注册到 `OPERATION_CATALOG` 或 `product.*` 子空间
│
└── 7.3.3 非目标
    ├── 不做主 GUI toolbar/strip
    ├── 不做终端内右键菜单
    └── 不做 settings 存储逻辑（存储仍在 server 侧 Rust）
│
7.4 Phase 3 — 远程更新通道
│
├── 目标
│   ├── 用户可获取新 pack 而无需重装 Base
│   ├── 签名验证 + 用户确认 + 失败回滚
│   └── 证据：更新 smoke 覆盖下载→staging→apply→rollback 四条路径
│
├── 7.4.1 更新流程
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
│   ├── ⑥ staging 目录：`~/.local/share/agenterm/app-pack-staging/`
│   └── ⑦ audit: `app_pack_update_downloaded` / `app_pack_update_applied` / `app_pack_update_failed`
│
├── 7.4.2 Native 新增
│   ├── `update.check(channel)` → manifest
│   ├── `update.download(manifest) → staging_path`
│   ├── `update.verify(staging_path) → bool`
│   ├── `update.apply(staging_path)` → 原子替换 + reload
│   └── `update.rollback()` → 恢复旧 pack
│
└── 7.4.3 非目标
    ├── 不做静默 overnight 替换
    ├── 不做无签名的 URL
    ├── 不做 pack 内自更新 bootstrap
    └── 不做增量/差分更新（全量 .agp 替换）
│
7.5 Phase 4 — 主 GUI chrome（远期，最后做）
│
├── 目标
│   ├── toolbar 行为策略、快捷键映射、右键菜单 → pack
│   ├── 仍 native 渲染（按钮、菜单、tooltip 像素），pack 只决定"显示什么"
│   └── 证据：同一 pack 在 Win/Unix 产生相同的 toolbar 语义
│
├── 7.5.1 候选迁移项
│   ├── toolbar action 映射（哪些 action 可见、顺序）
│   ├── 快捷键表（pack 声明，native 注册）
│   ├── 右键菜单项与顺序
│   ├── 空态欢迎页 copy
│   └── tab editor 校验规则与提示文案
│
└── 7.5.2 非目标
    ├── 不做终端网格内渲染
    ├── 不做字体/颜色渲染管线
    └── 不做窗口管理（最小化/最大化/DPI 策略）
│
7.6 QJS 前置工作 + rh 辅轨（独立于 Phase 时间线）
│
├── QJS-M6：`shipped_surfaces` 级 API 静态校验（Phase 0 前置条件）
│   ├── 对齐 `agenterm-rh/shipped_surfaces.rs` 的 76 条声明
│   ├── qjs `check` 增加 `--shipped-surfaces` 模式
│   ├── CI gate：qjs check 不得有 stale 声明
│   └── 这是 QJS 作为 product pack 引擎的"出厂资格"——在此之前 rh 仍是 Build/CI 唯一可用引擎
│
├── QJS-Embed：嵌入模式接入（Phase 0 同步做）
│   ├── `AppPackEngine` 按 manifest.engine 选择 ScriptEngineBackend
│   ├── QJS 的 `eval_entry_with_host` 接入长驻 Engine（目前是 run-to-exit）
│   ├── entry.js export function → native 回调解析
│   └── smoke：同内容 `entry.js` → CC footer_line 通过
│
├── QJS-Module：ES module import 跨目录引用
│   ├── `cc/nav.js` 可以 `import { fleet } from "../lib/fleet.js"`
│   ├── `module_resolver.rs` 已支持（agenterm-qjs/src/module_resolver.rs）
│   └── 验证：多模块 pack → 全部 resolve → eval 成功
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

## 8. 决策表

| 问题 | 选项 | 决定 | 理由 |
|------|------|------|------|
| v1 引擎 | Rhai 单轨 / QJS 单轨 / 双轨 | **QJS 进 app pack，rh 留 Build/CI** | QJS 跨平台源码一份、hot reload 秒级、CC Phase C WebView 互通；rh AOT 在构建 task 才是主场 |
| Pack 格式 | .dll / .js 源码 / 字节码 blob | **密封源码目录（tar.zst）** | 用户可读可改可 fork；跨平台一份；开发 edit→reload 秒级 |
| Pack 结构 | 单文件 / 多模块目录 | **按产品域分目录（cc/shell/settings/llm/theme/lib）** | 目录树 = 产品模块树；entry.js 是 native↔script 唯一接触面 |
| Pack 粒度 | 单 `agenterm.app` / 多独立 pack | **单 monorepo pack + 模块** | LLM 可拆子模块；避免多 pack 版本矩阵爆炸 |
| Engine 进程模型 | 内嵌 / `agenterm-rh` 子进程 | **内嵌** product engine | CLI 仍用独立 PE；pack 内嵌避免 IPC 延迟 |
| 远程更新默认 | 开 / 关 / 检查+静默下载+提示 | **检查 + 静默下载 + 提示加载** | 下载不打断用户；下载完成后非侵入提示；用户决定何时 reload |
| Pack 源码 | 开源随 repo / 闭源 channel | **随 repo 开源** | 内嵌出厂 pack 是 build artifact |
| CC 原生 PE | 保留 / 合并 | **保留** `agenterm-cc` thin PE | pack 内容驱动；PE 壳不变 |
| UX 统一目标 | 像素 / 语义 | **语义 + layout 契约** | 像素由 native/theme 保证 |
| 自解包格式 | tar+zstd / zip / 平面目录复制 | **tar+zstd** 密封归档 | 跨平台一致；`include_bytes!` 进 PE 资源段 |
| QJS 引擎在 pack 内的角色 | 替代 rh / 并行共存 / 远期选项 | **并行共存** | manifest.engine 选择；同一 catalog 共享 |
| 第一个 pack 内容范围 | 仅 CC / CC + LLM / 全产品 | **仅 CC** | CC 是独立 PE + composed lines，最适合 Strangler |

---

## 9. 风险目录（树）

```text
风险
├── 9.1 范围蠕变 🔴
│   ├── 症状：Phase 0 就想做 on_frame / toolbar / 远程更新
│   ├── 后果：自解包都没跑通就开始塞产品逻辑 → 全线崩塌
│   └── 对策：Phase 0 占位 pack ≤ 30 行 Rhai；Phase 1 只接一条竖线；gate 写死
│
├── 9.2 Host API 版本耦合 🟠
│   ├── 症状：pack 调 `fleet.tab.close`，Base 改了参数形状 → pack 静默行为错误
│   ├── 后果：更新 pack 后终端行为异常，用户不知是 pack 还是 Base 的问题
│   └── 对策：manifest.requires_base 窄区间；`app-pack doctor` 兼容性检查；CI 采样矩阵
│
├── 9.3 双调试栈 🟠
│   ├── 症状：bug 在 pack 脚本还是 native loader？栈追踪断在 FFI 边界
│   ├── 后果：排查时间翻倍；用户报告"CC 空态不对"无法定位
│   └── 对策：pack 行号映射 + 结构化 panic + `app-pack doctor` 诊断命令
│
├── 9.4 远程更新信任模型 🔴
│   ├── 症状：channel 被劫持 → 恶意 pack 下发 → 用户无感知
│   ├── 后果：远程代码执行；供应链攻击面
│   └── 对策：Publisher 密钥签名 + sha256 比对 + 用户显式确认对话框 + 离线可拒绝
│
├── 9.5 启动延迟 🟡
│   ├── 症状：首次启动解包 + Rhai AOT 编译 → 冷启动增加 200–500ms
│   ├── 后果：用户感知"变慢了"
│   └── 对策：rh AOT 预编译 native .dll 随 pack 发布（非首次编译）；QJS 无此问题
│
├── 9.6 Engine 内存 🟡
│   ├── 症状：多窗口 + 每窗一个 Engine → 内存线性增长
│   ├── 后果：10 窗 = 10× Engine 内存
│   └── 对策：单进程单 Engine + reload 纪律；pack 不持有大对象
│
└── 9.7 两套 truth 🟠
    ├── 症状：pack 缓存了 Fleet 状态 → server 侧变了 pack 不知道
    ├── 后果：CC 显示过期的 tab 列表
    └── 对策：pack 只投影 server snapshot；每次回调 native 传入最新 ctx
```

---

## 10. 证据门（每 Phase 出证）

| Phase | 证据类型 | 具体门 |
|-------|---------|--------|
| Phase 0 | smoke | `scripts/qjs/app-pack-smoke.js`：启动→pack loaded→reload→pack reloaded |
| Phase 0 | snapshot | `ui-snapshot` 含 `app_pack_version` 字段 |
| Phase 0 | CLI | `agenterm cli app-pack status` 打印 `version: 0.1.0, engine: qjs, path: ~/.local/share/agenterm/app-pack/` |
| Phase 0 | fallback | 无 pack 时 server 正常启动（Rust fallback，无 `app_pack_version` 字段） |
| Phase 0 | entry.js | `entry.js` export function 全部能被 native 正确调用并拿到返回值 |
| QJS-M6 | CI | qjs `check --shipped-surfaces` 不得有 stale 声明（Phase 0 前置条件） |
| Phase 1 | callback | pack 切换版本 → CC about 文案变化；pack 删除 → Rust fallback 同内容 |
| Phase 1 | metric | 单次回调 > 50ms → fallback + metric 记录 |
| Phase 2 | snapshot | CC snapshot 字段由 pack 驱动；native 只做 hit-test |
| Phase 2 | parity | 同一 pack 在 Win/Unix 产生相同 CC 语义（`cc-snapshot` diff） |
| Phase 3 | update smoke | 静默下载→校验→提示→apply 成功；apply 失败→rollback 恢复 |
| Phase 3 | signature | 篡改 pack → verify 失败 → 拒绝 apply；无签名 pack → 拒绝 |
| Phase 3 | endpoint | `https://agenterm.work/release/latest.json` 可达且 schema 兼容 |
| Phase 4 | toolbar | 同一 pack 在 Win/Unix 产生相同 toolbar 语义 |
| QJS-M6 | CI | qjs `check --shipped-surfaces` 不得有 stale 声明 |

---

## 11. 交叉引用

| 文档 | 关系 |
|------|------|
| `plan/agenterm-rhai-app.md` | 架构讨论稿 rev1；本文是它的执行投影 |
| `plan/design-rhai-rust-boundary.md` | L1/L2/L3 三层边界 SSOT |
| `plan/design-release-base-vs-apps.md` | Base vs Apps 分轨发布设计 |
| `plan/design-scripting-boundary-comparison.md` | Rhai/Lua/QJS 引擎边界对照 |
| `plan/design-script-engine-trait.md` | `ScriptEngineBackend` trait 设计 |
| `plan/ARCHITECTURE.md` | 现行结构 SSOT；三层边界 |
| `plan/plan-v0.1.17.md` | v0.1.17 收口版；本计划在其后执行 |
| `prd/PRD_02_10_rhai_scripting.md` | Script 引擎家族产品归属 |
| `prd/PRD_02_02_executable_family.md` | 可执行文件家族 |
| `plan/design-llm-gateway-rhai-logic-pack.md` | LLM Logic Pack（与 CC pack 可并行） |
| `plan/design-cc-hyper-control-agent.md` | CC 超控设计 |
| `docs/agenterm-rhai-runtime.md` | Script Runtime 用户文档 |

---

## 12. 版本列车对齐

```text
v0.1.17（收口版）
├── 主题：发布链证据 + 安装尾 + 脚本引擎深化 + 低成本卫生
├── 本计划占用：0（仅文档对齐）
└── 为 v0.1.18 做准备：`plan/agenterm-rhai-app.md` A0 定稿 + LLM pack A1

v0.1.18（建议：App Pack Phase 0）
├── 主题：QJS 占位 pack + 自解包 + 嵌入 Engine
├── 范围：本计划 §7.1 全部 + QJS-M6 收口
└── 交付：smoke 通过 + snapshot 含 app_pack_version + QJS shipped_surfaces 全绿

v0.1.19+（按 Phase 推进）
├── Phase 1：接一条竖线（QJS callback → CC footer）
├── Phase 2：CC chrome 全量迁移到 QJS pack
├── Phase 3：远程更新通道（agenterm.work/release/latest.json）
├── Phase 4：主 GUI chrome
└── rh 辅轨：Build/CI 持续投入，不进 app pack 路径
```
