# Plan tree: 薄 L1 内核 + 可换 L2 宿主 ABI + L3 应用包

| 字段 | 值 |
|------|-----|
| **状态** | active — 执行投影（不是结构 SSOT，不是产品真理） |
| **结构 SSOT** | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| **产品归属** | [`prd/PRD_02_10_rhai_scripting.md`](../prd/PRD_02_10_rhai_scripting.md) Layered deployment；[`prd/PRD_02_18_roadmap.md`](../prd/PRD_02_18_roadmap.md) M15 |
| **已有近亲（勿重开）** | [`plan-v0.1.18.md`](plan-v0.1.18.md) 轨 A（`.agp` = **L3**）；[`design-rhai-rust-boundary.md`](design-rhai-rust-boundary.md)（**脚本** L1/L2/L3）；[`design-release-base-vs-apps.md`](design-release-base-vs-apps.md)；[`plan-ape-thin-shell-dynamic-packages.md`](plan-ape-thin-shell-dynamic-packages.md)（编译拆分，不是热更） |
| **机器可读 L1 面** | [`shell-l1-surface.json`](shell-l1-surface.json) |

## 0. 一句产品结果

**尽快冻一层很薄的内核壳（Shell-L1）。**  
六格 Candidate / 签名公证只在这层变时才触发。  
资源清单、跨 OS 封装、computer-use 手、应用逻辑，都不得再靠「往 `agenterm.exe` 里加死代码」涨。

应用写一次三端跑 = 只调 **Shell-L2 能力名**。  
L2 像 JVM 标准库：常改、单独发，不重编 L1。  
L3 像 class/jar：包。

命名：**Shell-L1 / Shell-L2 / Shell-L3**。  
不要和脚本边界的 L1 kernel / L2 facade / L3 script 混称（那套里 L2 仍编进 Base PE，正是本树要拆开的）。

## 1. 为什么现在做

资源面（PTY、窗、AX/UIA、注入、文件……）是开集。  
全写成壳里的 Rust = 壳永远做不完 = 永远冲六格 CI。  
dyn 探路只该喂 **L2 的表/包**，不该每波改 L1。

成功不看又加了几个 libc 符号，看：

- L1 路径集合稳定、增长必须显式登记；
- 日常产品/能力改动可以不跑 Base Candidate；
- L3 不出现 OS 库名。

## 2. 三层（验收尺）

```text
Shell-L3  应用包（.agp / logic pack）     只调能力名；写一次三端跑
              │  Host ABI（versioned）
Shell-L2  应用壳包 / 宿主 ABI              跨 OS 封装、资源清单、cu 手
              │  加载 · 调用 · 失败码
Shell-L1  薄内核                           呈窗、PTY、IPC、加载器、有界跳板
```

| 层 | 装什么 | 不装什么 | 变更谁付钱 |
|----|--------|----------|------------|
| **L1** | 进程起来、窗呈上桌面、PTY 字节、IPC、包加载/校验骨架、**一种**有界原生跳法（签名/缓冲/失败） | 能力清单、Fleet 剧本、cu 策略、文案、Hub | **六格 CI + 签名公证** |
| **L2** | 能力名→落实（数据表、可移植包、每 OS 小插件/cu 进程） | 热路径 parser/blit、第二套 Fleet 权威 | **自己的门**（单 OS 或无 Cargo）；**禁止** Base Candidate |
| **L3** | 产品行为包 | `dlcall`、OS 库名、直接 platform | 换包；不编 PE |

L2 若仍是「再编一个跨平台 PE」，层是假的。

## 3. 能力树

### 3.1 点名并冻 L1（第一刀 · 加速开发的前提）

**用户问题：** 改一句能力/文案/cu 手也要全矩阵。  
**不变量：** 未登记进 [`shell-l1-surface.json`](shell-l1-surface.json) 的路径，**不得**成为「必须六格 Candidate」的理由。L1 涨路径 = 改该 JSON + 说明为什么跳板/呈窗/PTY/加载器不够。  
**证据：** JSON 存在且被本计划引用；后续 CI 叶只对 L1 面跑 Candidate（本波只点名，不改 Actions）。  
**失败：** 清单含糊，「整个 `src/` 都是 L1」。  
**非目标：** 本波不拆 crate、不改 `build.bat` 默认。

### 3.2 L1 薄化（只在跳法不够时动）

**行为：** L1 只保留呈窗/PTY/IPC/加载器/有界跳板。新 ioctl 码、新 Darwin 符号、新 cu 手 **默认进 L2**。  
**证据：** 新增 host 事实走 L2 表或 cu 包；L1 diff 不含「又一个 getpid 包装」。  
**非目标：** 把 `agenterm-platform` 从 workspace 删掉；platform **机制类型**仍可链进 L1，**资源清单**不当 L1 API 涨。

### 3.3 L2 宿主 ABI（JVM 标准库那一层）

**行为：** 应用只看见 versioned 能力名（现有 `fleet.*` / 未来 `cu.*` 是原料）。落实是数据或独立产物。  
**证据：** 一份机器可读 Host ABI；L3 夹具只调名字；换 L2 表/包不 `cargo build` L1 PE。  
**失败：** L2 更新走 `check.cmd --release --include-stress`。  
**近亲：** v0.1.18 的 App Host ABI 是 **L3→L2** 合同草稿；本树要求 L2 **本身**也能离开 Base zip。  
**非目标：** 本树第一年不把全部 `OPERATION_CATALOG` 搬出 PE。先冻结「哪些 id 允许 L3 用」，再搬落实。

### 3.4 computer-use 等带策略的资源

**行为：** AX / UIA / AT-SPI / 注入留在 **cu 进程/包（L2）**，工作台 L1 只保留「能唤起、能调能力、能收到 typed 失败」。  
**证据：** 多一个 cu 手 = cu 门，不是 `agenterm` 六格。  
**非目标：** 把 cu 焊回 `agenterm.exe`。

### 3.5 L3 应用包

**行为：** 一份字节，六格 Base 只验证+加载（v0.1.18 轨 A）。  
**证据：** 同 SHA `.agp`；App lane 无 Cargo。  
**非目标：** 远程静默 OTA、签名吊销（仍归 v0.1.18 非目标，直到单独授权）。

### 3.6 dyn 的位置

**行为：** 有界跳板属于 **L1 机制**（很少动）。符号/库名/探路结果属于 **L2 数据**。应用 **禁止** 直接 `dlcall`。  
**证据：** 新 Darwin 行不再改 L1 跳板；L3 包里没有 `libSystem.B.dylib`。  
**非目标：** 把 dyn 做成第四引擎或第二套 platform。

## 4. 依赖与波次

```text
W0 点名 L1 面（JSON + 本文 + ARCHITECTURE/PRD 挂钩）
      │
      ▼
W1  CI 意图：非 L1 路径变更不得要求 Candidate（先文档/门禁草案，后改 workflow）
      │
      ├── W2a  Host ABI 能力名冻结表（从 OPERATION_CATALOG 分类，不搬代码）
      └── W2b  cu 明确登记为 L2 产物（已独立 PE，只补合同）
      │
      ▼
W3  L2 可单独交付的第一份产物（数据表或包；无六格 Cargo）
      │
      ▼
W4  L3 只调冻结名（接 v0.1.18 `.agp`，不重开轨 A）
```

串行：W0 → W1 意图 → W3。  
W2a / W2b 可并行。  
W4 依赖 v0.1.18 轨 A 业主，本树不抢 `.agp` 构建器。

热文件（一次一个 owner）：`plan/ARCHITECTURE.md`、`Cargo.toml`、`.github/workflows/*`、`src/operations.rs`、`src/script_catalog.rs`。

## 5. 现行对照（今天还是一层 PE）

| 今天 | 目标 |
|------|------|
| `agenterm` PE 含内核+Facade+大量产品语义 | 拆清：PE 主责 L1；L2/L3 可换 |
| `OPERATION_CATALOG` 编进 Base | 先分类，后把落实移出 Candidate |
| `agenterm-cu` 已独立 | 正式叫 L2，改 cu 不冲工作台六格 |
| `agenterm-dyn` 探针潮 | 停用探针当版本主题；跳板留 L1 |
| `agenterm-platform` | L1 **用**它的机制类型；清单不当 L1 涨点 |
| v0.1.18 `.agp` | **L3**，不是 L1 |

## 6. 非目标（整棵树）

- Electron / 把 WebView 链进正式 `agenterm.exe`
- 应用里直接 `dlcall` / OS 库名
- 在 dyn 上做第二套跨系统封装
- 重开 ape 编译拆分当热更
- 把 Script L1/L2/L3 改名冲掉本树（两套名字并存，本文用 Shell- 前缀）
- 第一波就改完 Actions 矩阵或取消 stress 资格门

## 7. W0 交付（本增量）

- 本文 + [`shell-l1-surface.json`](shell-l1-surface.json)
- [`ARCHITECTURE.md`](ARCHITECTURE.md) 增加目标分层指针（不宣称已经拆完）
- [`README.md`](README.md) 索引
- PRD Layered deployment 链到本文

W0 成功：人能指出「改这个文件该不该跑六格」。  
W0 不改 CI、不拆 crate。
