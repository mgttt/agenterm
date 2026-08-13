# AgenTerm v0.1.18 公开计划

状态：**在制唯一版本计划**（2026-08-12 起）
不创建 tag / Candidate / Release，不触发公开更新，除非人工明确授权。
本文件是版本执行投影，不替代 PRD、结构 SSOT 或 App Pack 详细设计。

**主题：Portable App Substrate + 四条并行轨。**

v0.1.17 已于 2026-08-12 归档，其全部未完成叶按 `plan/README.md` §归档规则 2
**upsert 到本文件 §11**；`plan-libagenterm.md` 同日全文合并进 **§14**。
本版因此同时承载五条轨：

| 轨 | 范围 | 本文位置 | 独立 Gate |
|----|------|----------|-----------|
| **A. App Substrate** | 稳定 App Host ABI + 一份 QJS `.agp` 跨六格消费 | §0–§10 | G0–G5（§4） |
| **B. v0.1.17 承接树** | 多窗、跨主机证据、发布链、安装尾、脚本引擎债、低成本卫生 | §11 | 无，逐叶带证据合同 |
| **C. agenterm-con** | 第二产品：预算、独立对齐门、巨石切分、余量叶 | §12 | GC1–GC3 |
| **D. agenterm-cu** | 新立项：computer-use 底座设计与 `current` 档原型 | §13 | GD1–GD3 |
| **E. libagenterm** | 机制层动态库：ABI 设计与 Phase 0 形态判决 | §14 | Phase 0 四判据（§14.6） |

轨 A 只证明动态应用底座成立：同一份密封 `.agp` 能被现有六个 OS/ISA Base
携带、校验、加载和重载；修改 App 内容不要求重新编译六份 Base。首个真实产品语义
迁移、远程更新、WASM 计算扩展和 APE/多架构 loader 均不在本版实现范围。

**五条轨相互不阻塞**，各自有独立 Gate 与验收条件（§4、§8）。任一轨未达门只影响
该轨，不得因此把另一轨标为完成，也不得借另一轨的绿状态替代本轨证据。

**轨 A 与轨 E 是两条不同的 ABI，永不合并**：轨 A 的 Host ABI 是产品语义边界
（App guest ↔ 产品），轨 E 的 `agt_*` 是机制边界（产品 ↔ OS）。见 §14.4。

> 结构 SSOT：[`ARCHITECTURE.md`](ARCHITECTURE.md)。
> 上版收口树（已归档，未完成叶见本文 §11）：
> [`archive/plan-v0.1.17.md`](archive/plan-v0.1.17.md)。
> 跨目标机制研究：[`reference-cross-target-execution.md`](reference-cross-target-execution.md)。
> 原 App Pack 讨论与分期推演已归档为
> [`archive/plan-agenterm-app-pack.md`](archive/plan-agenterm-app-pack.md)；本文件已吸收其仍生效的
> 架构合同、Phase 0 执行叶和后续去向。

---

## 0. 版本结果与边界

### 0.1 唯一产品结果

v0.1.18 完成时，仓库必须能给出下列可复现证据：

```text
一份 agenterm-app-<version>.agp（一个 SHA-256）
              │
              ├── win  × x86_64 Base
              ├── win  × aarch64 Base
              ├── lnx  × x86_64 Base
              ├── lnx  × aarch64 Base
              ├── osx  × x86_64 Base
              └── osx  × aarch64 Base

修改 entry.js / manifest 内容
              │
              └── App lane 构建、校验、合同测试通过；不重新编译 Base 六格
```

“一份编写、到处运行”在本版特指**产品应用包与产品语义**，不表示一份原生机器码
覆盖全部 OS/ISA。窗口、PTY、输入、IME、渲染、IPC 和进程机制继续由各目标的
Native Base 与 `agenterm-platform` 承担。

### 0.2 完成定义

本版的“完成”不是“QJS 能编译”，而是以下三件事同时成立：

1. App 与 Base 之间存在窄小、版本化、typed fail-closed 的 Host ABI v1。
2. 同一 `.agp` 字节身份被六格 Base 消费，且本机可执行格有真实加载/重载证据。
3. App-only 改动走不调用 Cargo、不重建 Native Base 的独立权威 lane。

### 0.3 轨 A 前置条件

轨 A（App Substrate）建立在可信 Base 之上，因此以下四项必须先由 **§11 承接树**
交付；它们现在是本版内部依赖，不再是"上一版是否收口"的外部条件：

| 前置 | owner 叶 |
|------|----------|
| capability/catalog 不再漂移 | §11 QJS-M6、E3、DOC-PRD |
| exact-SHA CI 与发布链无活跃红 | §11 R1e/R2e/R4e、T-debt |
| `agenterm cli script` 已删除，公开入口为 `agenterm rh\|lua\|qjs\|sql` | §11 E5 |
| 测试不遗留锁住构建产物的 `agenterm server` 孤儿进程 | §11 E4 |

任一前置不满足时，**轨 A 保持"未开工"**，不得用 App Pack 工作绕过 Base 的不可信
状态。轨 B/C/D 不受此约束，可并行推进。

---

## 1. App Pack 架构合同（由原专题计划收敛）

### 1.1 目标形态与永久原生边界

```text
Native Base（每 OS/ISA 一份，低频变化）
├── Server / Fleet 权威
├── PTY / parser / IPC / journal
├── 原生窗口、输入、IME、剪贴板与渲染
├── App Host ABI + QJS Engine + pack loader
└── 内嵌一份 factory `.agp`

agenterm.app（一个跨 OS/ISA `.agp`，高频变化）
├── manifest.json
├── entry.js（native ↔ App 唯一接触面）
├── 按产品域组织的 ES modules
└── 文案、声明式产品语义和后续可迁移策略
```

PTY/ConPTY、parser、blit、Fleet 权威、IPC 传输、journal、OS handle 和平台 adapter
永久留在 Native Base。App 只能消费宿主传入的 typed snapshot，并返回产品语义；不得缓存
Fleet 形成第二权威，也不得进入逐帧渲染或字节级热路径。

“Thin Base”表示**产品策略逐步变薄且变化频率降低**，不表示 v0.1.18 已经产出跨平台单一
native executable。现有六格 Base 与签名/公证合同保持不变。

### 1.2 三层命名与包内容

| 层 | 名称 | 含义 |
|----|------|------|
| 产品概念 | `agenterm.app` / App Pack | 动态产品应用层 |
| 分发文件 | `agenterm-app-<version>.agp` | 确定性 tar+zstd 密封归档 |
| 运行副本 | `<产品数据目录>/agenterm/app-pack/` | 经 platform path policy 解析后的解包目录 |

`.app` 不作为文件扩展名，避免与 macOS application bundle 冲突。文档和代码不得硬编码
某一平台的数据根；统一通过现有 product-directory policy 获取。

v1 包内容固定为源码文件树，不含 `.dll`、`.so`、`.qjsc`、`.wasm` 或 `.cwasm`：

```text
agenterm-app-<version>.agp
├── manifest.json
├── entry.js
├── cc/          # 后续版本才迁真实内容
├── shell/       # 后续版本
├── settings/    # 后续版本
├── llm/         # 后续版本，可再决定是否拆包
├── theme/       # 后续版本
└── lib/         # App 内共享模块；不是第二套 Host API
```

本版占位 `entry.js` 保持极小，只导出版本与只读测试值。目录按产品域预留不等于这些产品域
已迁移或已获得 must-ship 承诺。

### 1.3 启动、来源与回滚模型

```text
Base 启动
├── 读取 pack 外的 app-pack.state.json
├── 校验本地 pack 身份与 Host ABI
├── 无 pack
│   └── 原子解出内嵌 factory pack
├── origin=factory 且内嵌版本更新
│   └── 原子替换为新 factory pack
├── origin=user
│   └── 不覆盖；status/doctor 提示显式 factory-reset
├── origin=ota
│   └── 本版只识别状态，不产生 ota；未来由更新 channel 管理
└── 加载成功
    └── 构造 server 进程唯一的长驻 Engine
```

`app-pack.state.json` 位于密封包外，至少保存 origin、installed hash 与安装身份；否则用户修改
pack 时会同时改变来源记录。写入、替换和 factory-reset 必须使用 owned staging + atomic replace，
失败保留上一份已知良好 pack。用户本地 pack 是显式选择在其用户权限下执行的代码；远程 pack
未来必须另有签名与来源合同，不能把“本地可改”误写成远程信任。

### 1.4 不变量与禁令

| ID | 不变量 |
|----|--------|
| **I1** | PTY/parser/blit 和逐帧渲染永不脚本化。 |
| **I2** | Server/Fleet 是唯一权威；App 不缓存动态 Fleet 状态。 |
| **I3** | IPC 传输与协议机制留在 Native Base。 |
| **I4** | OS 差异止于 `agenterm-platform`/host adapter；App 无平台 cfg/handle。 |
| **I5** | Phase 0–1 保留等价 Rust fallback；Phase 2 起只保证可诊断最小安全态。 |
| **I6** | App Host API 是兼容边界，不是权限 sandbox；`capability` 仅为发现元数据。 |
| **I7** | 不引入 npm 或传递依赖求解；`.agp` 整包密封替换。 |
| **I8** | entry.js 是 native 与 App 的唯一模块接触面；内部目录可独立重构。 |
| **I9** | 不静默远程替换；远程下载、签名、确认和回滚留待后续 Phase。 |
| **I10** | 不新增 App Pack 独立 PE；Engine 内嵌 server，`agenterm-cc` 经 IPC 取静态语义。 |
| **I11** | 能调宿主 API 的签名远程 pack 等价于用户权限下代码执行；签名是供应链边界，不是 API 权限。 |
| **I12** | 连续失败达到有界阈值后禁用当前 pack，进入可见、可恢复的最小安全态。 |

I5/I12 的阶段口径不得混用：Phase 0 的 pack 没有真实产品 authority，缺失或失败时 Base
现有 Rust 行为完全不变，只增加稳定诊断状态；Phase 1 的首条竖线保留内容等价 fallback；
只有 Phase 2 删除对应 Rust authority 后，才允许降级到可诊断的最小安全态。连续失败熔断的
机制底座可在本版实现，但 persisted disabled、可见提示和 doctor 恢复证据由 Phase 1 首条
真实回调负责，不能用占位 pack 假装已经验证产品降级。

### 1.5 引擎与进程模型

- QJS 是 v1 App Engine：源码一份、reload 快、适合低频产品回调与未来 WebView 语义复用。
- Rh 继续拥有 Build/CI、qualification、smoke 和通用本地自动化，不进入 App 长驻路径。
- Lua/SQL 保留各自公开 CLI 能力，不参与 App Engine 竞争。
- WASM 是后续可选计算模块，不是本版第三 App Engine，也不替代 QJS/Rh。
- server 进程只有一份长驻 QJS Runtime/Context；多个 GUI/CC client 不各建 Engine。
- `agenterm-cc` 通过已有 IPC 拉取可缓存的静态 App 语义；server reload 后发送失效通知。
  不允许 CC 逐帧 IPC 调脚本，也不允许缓存 Fleet snapshot 形成第二权威。

manifest v1 固定 `engine=qjs`。保留 engine 字段是格式演进点，不表示本版实现多引擎 App。

### 1.6 可复用基础与本版缺口

| 类别 | 已有基础 | 本版仍需交付 |
|------|----------|--------------|
| QJS pack | source/hash/manifest、module resolver、host bridge、CLI pack/check/eval | 默认宿主采用门、长驻 Runtime/Context、具名 export、interrupt |
| Script common | hash、receipt、check-many 等共享实现 | `.agp` manifest/文件集 verifier 与 Host ABI 对账 |
| Product glue | Script backend/engine trait、QJS/Rh host、公共 operation catalog | 独立 `AppPackEngine` facade，不让产品调用裸引擎 API |
| Platform paths | product data root policy 与 boundary tests | app-pack/staging/state 路径全部经 policy |
| Lifecycle | Rh pack 环境变量加载先例 | factory 内嵌、自解包、origin、reload、doctor、factory-reset |
| Observation | CLI/snapshot/event journal 基础 | 稳定 `app_pack` 状态、typed error 与 reload 事件 |

现有机制是复用起点，不代表产品路径已经接线。尤其 QJS 当前 run-to-exit pack 求值、可选 feature
和 wasmcore 独立实验都不能被描述为 App Pack 已经可用。

### 1.7 Phase 0 实现切片

本版 Phase 0 只实现以下纵向链路，且必须遵守后文 Gate：

1. 规范化 `manifest.json` 与 `.agp` builder/verifier。
2. 构建极小 factory `entry.js`，由 Base 构建输入确定性内嵌。
3. `AppPack::load_or_extract()` 经 platform policy 完成三态判断和原子落盘。
4. `AppPackEngine` 建立单 Engine、具名 export、typed value、interrupt 与 dirty reload。
5. 公共 CLI 提供 `app-pack status|doctor|reload|factory-reset`；extract 是内部生命周期动作，
   如保留公开诊断入口也不得绕过 verifier。
6. snapshot 始终输出稳定 `app_pack` 对象。
7. 六格 Base 消费同一 `.agp`；App-only lane 独立构建/校验且不调用 Cargo。

Phase 0 明确不调用 `fleet.*`，不迁 CC 文案，不实现远程下载，也不把占位 export 当作真实产品
authority。Phase 0 的价值是证明边界和交付解耦，而不是展示脚本能够生成多少 UI。

### 1.8 风险与本版控制

| 风险 | 本版控制 | 后续 owner |
|------|----------|------------|
| QJS 静态进入 Base 增加六格编译/体积 | Q0a 先测量；超预算停止，不自动换 Rh App | Runtime Component 方案评估 |
| Host API 随 Base 漂移 | ABI version + required operations + fixture matrix | 每次 ABI 变更的 Base/App compatibility gate |
| 双调试栈 | typed error、回调名、源位置、doctor、event | Phase 1 可观测性 |
| Engine 跑飞或半更新 | interrupt + dirty Engine 整体重建 + 有界熔断 | QJS embed owner |
| 两套状态真相 | App 只投影 ctx；CC 只缓存静态语义 | Phase 2 IPC/snapshot parity |
| 本地修改被覆盖 | pack 外 origin/hash + factory-reset | L0 lifecycle |
| 远程代码供应链 | 本版不联网；未来公钥、签名、确认、吊销、回滚成组交付 | Phase 3 |
| pack/prev/staging 堆积 | 本版只拥有 factory/user 与有界 staging；远程代际策略后置 | Phase 3 disk lifecycle |

### 1.9 后续 Phase 去向（不丢叶、不提前执行）

| 原专题 Phase | 建议版本 | 仍需交付的完整结果 |
|--------------|----------|--------------------|
| **Phase 1** | v0.1.19 | 首条真实 CC 静态语义竖线；typed callback、等价短期 fallback、interrupt、熔断、event；迁完一块删一块 Rust 重复 |
| **Phase 2** | v0.1.20+ | CC nav/empty/settings 静态语义；CC 经 IPC 缓存并响应 reload invalidation；进入本 Phase 时 fallback 改为最小安全态 |
| **Phase 3** | v0.1.20+ 独立授权 | signed channel、静默下载但显式 apply、staging、atomic rollback、密钥轮换/吊销与磁盘代际上限 |
| **Phase 4** | v0.2.x | 主 GUI toolbar/shortcut/context-menu 等声明式语义；Win/Unix 同 pack parity；仍不进入终端网格渲染 |
| **WASM 扩展** | v0.1.20+ 实验 | 独立 guest ABI 与真实性能场景；默认只作计算模块，不接管 product authority |
| **多架构 loader/APE** | v0.2.x 研究门 | 只优化交付封装；不得声称替代 Host ABI、ISA 机器码、PE、macOS 签名或平台 adapter |

这些去向是完整叶的保留位置，不是相应版本已承诺 must-ship。建立后续版本计划时必须重新展开
用户问题、不变量、证据、安全失败、owner 与非目标，不能只复制 Phase 名称。

### 1.10 长期权威落点

本文件冻结执行顺序；长期产品合同必须在实现相应叶时同步到以下 owning 文档：

| 合同 | owning 文档 |
|------|-------------|
| QJS product App、Rh Build/CI、Host ABI 与 failure 语义 | [`../prd/PRD_02_10_rhai_scripting.md`](../prd/PRD_02_10_rhai_scripting.md) |
| 不新增 App Pack PE、server 单 Engine、CC 经 IPC 取静态语义 | [`../prd/PRD_02_02_executable_family.md`](../prd/PRD_02_02_executable_family.md) |
| Base/App lane、单一 SHA、provenance 与六格证据等级 | [`../prd/PRD_02_17_delivery_quality.md`](../prd/PRD_02_17_delivery_quality.md) |
| CC 静态语义、IPC cache/invalidation、fallback 阶段与 parity/i18n | [`../prd/PRD_02_21_control_center.md`](../prd/PRD_02_21_control_center.md) |
| Phase 0–4 版本去向 | [`../prd/PRD_02_18_roadmap.md`](../prd/PRD_02_18_roadmap.md) |
| 平台数据根机制 | [`../prd/PRD_02_20_native_platform.md`](../prd/PRD_02_20_native_platform.md)；不得接管 App 产品 policy |

实际新增模块、pack source 目录或 CI lane 时同步 `ARCHITECTURE.md`；不得在版本 plan 另造第二份
living file map。能力状态落地时再同步 `prd/alignment-contract.json` 和公共 catalog，不在草案阶段
预先虚报 shipped。

---

## 2. 依赖树（轨 A）

```text
P0  v0.1.17 基线冻结
│
├── H1  App Host ABI v1
│   ├── H1a manifest 身份与兼容合同
│   ├── H1b 最小 product/runtime surface
│   └── H1c typed failure + snapshot schema
│
├── Q0  QJS 宿主采用门
│   ├── Q0a 六格工具链与依赖测量
│   ├── Q0b 长驻 Runtime/Context + interrupt
│   └── Q0c ES module 根边界
│
└── A0  `.agp` 确定性构建与校验
    ├── A0a 密封文件树
    └── A0b 单一 SHA / provenance

H1 + Q0 + A0
      │
      ▼
L0  factory pack 生命周期
├── extract / status / doctor / reload / factory-reset
├── factory|user|ota 状态模型（本版只产生 factory/user）
└── reload 不杀 PTY/server/lease
      │
      ▼
X0  跨六格消费 + App-only 无 Cargo 决定性证据
```

共享热边界：`Cargo.toml`、Script backend/engine、公共 snapshot schema、构建与 CI
入口必须由主线串行集成；不得让不同 owner 并发改写。`.agp` 构建器、QJS Engine
内部和 CLI 黑盒可在接口冻结后按独占文件集并行。

---

## 3. 可执行工作树（轨 A）

每个叶均包含：用户问题、不变量、证据、安全失败、黑盒 owner 与非目标。
其余轨的工作树在 §11（B）、§12（C）、§13（D）、§14（E）。

### P0. 基线冻结

- [ ] **P0 轨 A 基线快照**
  - **用户问题**：动态底座不能建立在仍漂移的 Base、catalog 或 CI 红之上。
  - **不变量**：只消费 §0.3 表中四项已证明的交付；未完成项保留原 owner 与去向。
  - **证据 / owner**：§11 对应叶的验收证据、exact-SHA CI 结论和 PRD capability
    对账共同拥有。
  - **安全失败**：任一前置仍活跃则停止轨 A 产品代码工作（不影响轨 B/C/D）。
  - **非目标**：不在轨 A 内重做 §11 的发布链或 GUI 尾账——那是轨 B 自己的工作。

### H1. App Host ABI v1

- [ ] **H1a manifest 身份与兼容合同**
  - **用户问题**：只用 Base semver 猜兼容性会让旧 Base 静默加载不兼容 App。
  - **不变量**：manifest 至少绑定 schema、App version、engine、Host ABI 范围、entry、
    所需 operation IDs、逐文件 hash、整包 hash 与 provenance；未知必填字段或不兼容 ABI
    必须 fail-closed。
  - **证据 / owner**：共享 manifest parser/validator 的 fixture 覆盖正确、缺字段、篡改、
    ABI 过新/过旧、未知 operation 和额外文件；`.agp` verifier 是唯一黑盒 owner。
  - **安全失败**：拒绝加载并保留当前已知良好 factory pack，不猜文件名或 API。
  - **非目标**：本版不定义远程 channel、签名密钥轮换或增量更新协议。

- [ ] **H1b 最小 Host ABI surface**
  - **用户问题**：把 Rh 的全部 surface 或原生结构直接暴露给 App 会制造永久兼容债。
  - **不变量**：v1 复用现有 Script host、typed bridge 与 catalog，外加窄小、版本化的
    App facade；不得建立第二套 runtime/host API。只暴露 `runtime.*`、只读 `product.*`
    占位回调和必要的结构化诊断；
    不暴露 OS handle、平台 cfg、Fleet 状态副本或逐帧渲染入口。`capability` 只表示发现与
    兼容元数据，不表示授权、拒绝或 sandbox。
  - **证据 / owner**：版本化 ABI catalog 与 QJS literal checker 一一对应；已知调用通过，
    未知 literal typed fail-closed，动态表达式诚实标为不可静态证明。
  - **安全失败**：缺 surface 时拒绝 pack、报告精确 operation ID，并保持 Phase 0 的现有
    Rust 产品行为不变；最小安全态只从 Phase 2 起适用。
  - **非目标**：不要求 QJS 复制 Rh 的全部 shipped surfaces；不迁真实 CC/Fleet 产品行为。

- [ ] **H1c 稳定状态与错误 schema**
  - **用户问题**：pack 缺失、禁用或不兼容时删除 snapshot 字段会破坏公共消费者。
  - **不变量**：snapshot 始终报告 `app_pack.state`、nullable version/origin 和 typed
    `last_error`；状态至少覆盖 loaded、disabled、unavailable、incompatible。
  - **证据 / owner**：公共 CLI/snapshot 黑盒覆盖四态与 schema 兼容。
  - **安全失败**：状态未知时报告 unavailable，不把缺字段解释成旧版成功。
  - **非目标**：不在本版设计完整线上遥测系统。

### Q0. QJS 宿主采用门

- [ ] **Q0a 工具链、体积与墙钟测量**
  - **用户问题**：把 QuickJS C 源码静态加入默认 Base 可能加重六格冷编译，抵消迭代收益。
  - **不变量**：在决定宿主形态前，记录六格可构建性、Base 体积、冷编译墙钟、增量墙钟、
    third-party notice 和启动增量；不恢复全局 Cargo jobs 限制。
  - **证据 / owner**：同一 exact source 的 before/after matrix 与构建计时摘要拥有该决定。
  - **安全失败**：任一格不能构建或超出既有发布预算时停止 Phase 0；本版只允许修工具链
    或缩减不影响唯一结果的附属范围。独立 Runtime Component 必须另立设计和版本，不能在
    Gate 内临场替换宿主形态后仍宣称 v0.1.18 完成。
  - **非目标**：不得自动回退到 Rh App Pack；其目标相关 AOT 产物不满足“一包六格”。

- [ ] **Q0b 长驻 QJS Runtime/Context**
  - **用户问题**：当前 run-to-exit 求值不能支持 App 生命周期和不终止 PTY 的 reload。
  - **不变量**：server 进程一份 Engine；回调有预算、取消和 interrupt；中断后 Engine
    标脏并整体重建，不继续使用可能半更新的状态。
  - **证据 / owner**：QJS embed 黑盒覆盖 load、具名 export、重复调用、死循环 interrupt、
    dirty reload 与旧 Engine 资源释放。
  - **安全失败**：失败记录稳定诊断并保持 Phase 0 现有 Rust 产品行为；不退出 server，
    不关闭 tab，不破坏 lease。最小安全态只从 Phase 2 起适用。
  - **非目标**：`agenterm-cc` 不自建第二份 Engine；不做逐帧脚本回调。

- [ ] **Q0c ES module 根与确定性加载**
  - **用户问题**：多模块 App 需要稳定 import，同时不能因工作目录不同加载不同文件。
  - **不变量**：所有相对 import 以 pack 根解析；`..` 不得逃出 pack 根；同一密封文件树
    产生同一模块图和 hash。这里是数据完整性边界，不是 Script Runtime 路径权限政策。
  - **证据 / owner**：QJS module resolver 黑盒覆盖嵌套、循环、缺模块、越界与大小写差异。
  - **安全失败**：整个 pack 拒绝加载，不执行半张模块图。
  - **非目标**：不做动态 `import()`、npm、网络模块或 WebView 共用。

### A0. `.agp` 确定性产物

- [ ] **A0a 密封源码包**
  - **用户问题**：目录复制和临时文件会导致不同主机产生不同 App 身份。
  - **不变量**：`.agp` 是确定性的 tar+zstd 文件树；排序、时间戳、权限、路径分隔符和
    manifest 序列化均规范化；v1 固定 `engine=qjs`，字段只为未来演进保留。
  - **证据 / owner**：相同输入连续构建两次字节相同；解包再验证得到相同文件集合/hash。
  - **安全失败**：重复路径、绝对路径、父目录逃逸、额外文件或 hash 漂移均拒绝封装/加载。
  - **非目标**：不包含 `.qjsc`、`.cwasm`、native library 或按目标分叉的内容。

- [ ] **A0b provenance 与单一身份传播**
  - **用户问题**：六格各自重建 App 会产生六个“看似相同”的包，无法证明一份到处运行。
  - **不变量**：App lane 只构建一次 `.agp`；六格只下载并验证同一 SHA，不得各格重建。
  - **证据 / owner**：matrix summary 输出相同 archive SHA、manifest SHA 和 source identity。
  - **安全失败**：任一格 SHA 不同即整体验证失败，不以内容抽样代替逐字节身份。
  - **非目标**：本版不把 `.agp` 发布为公开 Release asset。

### L0. factory pack 生命周期

- [ ] **L0a 自解包与三态来源**
  - **用户问题**：Base 升级既不能永远留下旧 factory pack，也不能覆盖用户本地修改。
  - **不变量**：使用平台路径 policy；状态位于密封 pack 外；factory/user/ota 三态合同冻结，
    本版只产生 factory 和显式 user，ota 仅保留 schema 值。
  - **证据 / owner**：首次解包、factory 升级、user 不覆盖、损坏状态和 factory-reset 黑盒；
    `platform::boundary_tests` 同时证明 App Pack 模块没有平台 cfg、原生 marker 或硬编码数据根。
  - **安全失败**：无法判定来源时不覆盖现有目录，doctor 给出恢复动作。
  - **非目标**：不下载远程 pack，不自动把本地编辑标成可信远程更新。

- [ ] **L0b status / doctor / reload / factory-reset**
  - **用户问题**：用户需要从公共入口判断正在运行哪份 App 以及如何恢复。
  - **不变量**：所有操作经 `agenterm cli app-pack ...`；路径由 platform policy 返回；
    reload 原子切换，失败保留上一份已知良好 Engine/pack。
  - **证据 / owner**：隔离 instance 的 CLI 黑盒覆盖每个命令、退出码和 snapshot 变化。
  - **安全失败**：reload 失败不终止 PTY/server/lease；factory-reset 不删除非本功能拥有的文件。
  - **非目标**：不增加独立 App Pack PE，不恢复 `agenterm cli script`。

  factory extraction 是启动生命周期的内部原子动作，本版不增加公开 `extract --force`。显式恢复
  统一走可审计的 `factory-reset`，避免一个旁路命令绕过 origin/verifier 合同。

- [ ] **L0c 占位 entry 与调用往返**
  - **用户问题**：仅能解包但不能从 native 调用具名 export，不能证明动态应用边界成立。
  - **不变量**：占位 App 不访问 Fleet、不改变产品行为；只返回版本与测试用只读值。
  - **证据 / owner**：native→QJS export→typed result 的重复调用与 reload 后版本变化黑盒。
  - **安全失败**：类型不匹配、缺 export 或异常均进入 H1c 状态并保持 Base 可用。
  - **非目标**：不把 CC footer、导航、toolbar 或设置 authority 迁入 pack。

### X0. 决定性解耦证据

- [ ] **X0a 六格消费同一 `.agp`**
  - **用户问题**：跨平台构建成功不等于同一动态 App 真被每个平台消费。
  - **不变量**：六个 Base archive 均包含或配对同一 `.agp` 字节；可原生执行的 OS/ISA 格
    必须真实 load/reload，交叉编译格至少完成 parser、manifest、archive member 和 ABI 合同验证，
    不把 existence-only 冒充 native execution。
  - **证据 / owner**：App compatibility matrix 汇总每格证据等级与同一 SHA。
  - **安全失败**：缺原生主机证据明确标 unresolved，不虚报六格运行完成。
  - **非目标**：不要求在当前主机模拟不可用的真实 GUI/PTY。

- [ ] **X0b App-only lane 不调用 Cargo**
  - **用户问题**：若改一句 JS 仍触发六格 Rust 编译，本版本没有解决迭代瓶颈。
  - **不变量**：只改 App 源码/manifest 时，权威 lane 仅做 pack build、lint、ABI 合同、
    fixtures 和已有 Base compatibility；不得调用 Cargo、重建 Base 或伪装复用旧编译为新编译。
  - **证据 / owner**：CI workflow contract + 一次真实 App-only 变更 run 的步骤/墙钟摘要。
  - **安全失败**：无法取得可信 Base fixture 时 typed skip 并要求 Base lane，不能悄悄少测。
  - **非目标**：Native、Host ABI、engine 或 platform 改动仍必须跑完整 Base 六格。

---

## 4. Gate 与执行顺序

| Gate | 必须证明 | 不通过时 |
|------|----------|----------|
| **G0 Base ready** | P0 全部前置已冻结 | 不开工 |
| **G1 ABI frozen** | H1 manifest/surface/state schema fixture 全绿 | 不写 loader |
| **G2 QJS adoption** | QJS 宿主进入现有六格 Base；Q0a 构建、预算、notice、墙钟有实数 | 停止本版本 Phase 0；替代宿主形态另立设计，不回退 Rh App |
| **G3 minimal load** | Q0b/c + A0 + L0 本地黑盒全绿 | 不建立 App-only lane |
| **G4 portability** | X0a 同一 SHA 六格证据诚实齐备 | 不宣称“一包六格” |
| **G5 decoupling** | X0b 真实 App-only run 不调用 Cargo | 不把 v0.1.18 标为完成 |

严格顺序：`G0 → G1 → G2 → G3 → G4 → G5`。G1 冻结后，QJS Engine 与
`.agp` builder 可并行；公共 schema、根 manifest、workflow 和 Script dispatch 属于集成热区，
由主线串行修改。最终 lint、Quick、Base matrix 与 App-only lane 在同一集成状态上串行验收。

**上表只管轨 A。** 轨 C 的 GC1–GC3 见 §12，轨 D 的 GD1–GD3 见 §13，
轨 E 的 Phase 0 四判据见 §14.6；轨 B 无独立
Gate，其叶各自带证据合同。五条轨的 Gate 互不替代：`G5` 通过不代表 con 预算达标，
`GC1` 全绿也不代表 App lane 成立，Phase 0 通过更不代表任何产品已迁入动态库。

---

## 5. CI 与证据分层

```text
App lane（高频）
├── JS lint / QJS check
├── manifest + Host ABI static validation
├── deterministic `.agp` build
├── tamper / traversal / compatibility fixtures
├── loader contract against frozen Base fixtures
└── 明确断言：未调用 Cargo

Base lane（低频）
├── Rust/QJS host/platform 变化才触发
├── 六 OS/ISA build + existing owning tests
├── 每格验证同一 `.agp` SHA
└── 原生可执行格执行 load/reload smoke

Candidate / Promotion
└── 仍遵守现行 exact-SHA 两阶段合同；本版不自行派发
```

App lane 不是“零 CI 成本”，而是“零 Base 重编译”。签名、远程来源、更新回滚进入
后续 Phase 时，必须在 App lane 上增加相应 supply-chain owner。

---

## 6. 明确非目标

- 不实现 APE、polyglot executable、跨 ISA 原生 loader 或单一万能二进制。
- 不把 `agenterm-platform` 变成产品 UI；OS 差异仍止于机制 crate/host adapter。
- 不接入 `agenterm-wasmcore`，不发布 `.wasm`/`.cwasm`，不把 QJS 或 Rh 编译到 WASM。
- 不迁移真实 CC 导航、空态、toolbar、settings、LLM 路由或逐帧渲染逻辑。
- 不做远程 channel、静默下载、签名密钥、吊销、apply/rollback 或公开 `.agp` Release。
- 不实现 QJS 动态 `import()`、npm、网络模块、WebView 共用或字节码缓存。
- 不暴露全量 Rh/Fleet surface 给 App；不把 robustness budget 描述成权限 sandbox。
- 不删除 Rh、Lua、SQL；Rh 继续拥有 Build/CI 与通用本地自动化。
- 不改变现行六平台 Base 发布合同，不因 App Pack 降低 Candidate/Promotion 验证强度。

轨 B/C/D 的非目标随叶登记；跨轨另加三条：

- 公开 **tag / Candidate / Promotion**（除非另文授权）。
- `agenterm-cu` 任一远程 tier（ssh/rdp/vnc）的实现，以及 cu 可执行体的公开发布。
- 新脚本引擎（SQL 之后的下一个）开工；回退 M22f 默认 rh backend。

---

## 7. 后续版本接口

| 后续 | 建议范围 | 本版必须留下的稳定接口 |
|------|----------|--------------------------|
| **v0.1.19** | Phase 1：一条真实 CC 静态语义竖线 | typed callback、等价 fallback、interrupt、event、persisted disabled 熔断和 doctor 恢复 |
| **v0.1.19+（独立授权）** | ape + thin shells 架构重构 | Phase A：拆分 `crates/agenterm-ape/`，将根 crate 的 ~110 个产品逻辑文件搬入独立 crate，~55 个平台薄壳文件留在根 crate。详见 [`plan-ape-thin-shell-dynamic-packages.md`](plan-ape-thin-shell-dynamic-packages.md)。 |
| **v0.1.20+** | Phase 2：CC 静态语义扩面 | nav→empty/settings→layout 顺序、IPC cache/invalidation、fallback 切最小安全态、Win/Unix parity 与 i18n |
| **v0.1.20+（独立授权）** | Phase 3：签名更新 | channel、离线公钥、显式 apply、rollback、audit、prev 一代、staging 上限与密钥轮换/吊销 |
| **v0.2.x** | Phase 4：主 GUI chrome | toolbar/shortcut/context-menu/welcome/tab-editor 声明语义；native 仍渲染 |
| **v0.2.x+** | QJS/WebView 语义复用评估 | 先证明同一模块在两宿主的 API/错误/生命周期语义，不在 Phase 0–4 偷渡 |
| **v0.1.20+** | WASM 计算扩展实验 | 与 QJS 正交的 guest ABI；不得接管 product authority |
| **v0.2.x** | 多架构薄壳/安装 loader 评估 | Base/App 分轨与单一 `.agp` 身份 |

APE 只能作为未来交付封装机制候选重新评估，不能替代 Host ABI、macOS 签名、Windows PE、
ISA 机器码或平台 adapter。WASM 首选定位是 App Pack 的可选计算模块；QJS 负责高频产品语义，
Rh 负责 Build/CI，Rust/Base 负责权威状态与原生机制。

---

## 8. 验收总门

未授权公开发布时，**开发完成** = 四条轨各自成立。轨与轨之间不互相顶替。

### 8.A 轨 A（App Substrate）

1. P0 基线快照冻结；§0.3 的四项前置由 §11 交付且无活跃红。
2. Host ABI v1、manifest 和稳定 snapshot schema 已进入 owning PRD/catalog，fixture 全绿。
3. QJS 宿主六格可构建，体积、冷/热墙钟、notice 和发布预算有实际证据。
4. `.agp` 确定性构建、hash/provenance、篡改与路径逃逸测试全绿。
5. status/doctor/reload/factory-reset 通过公共 CLI；失败不杀 PTY/server/lease。
6. platform boundary test 证明 App Pack 代码没有平台 cfg、原生 marker 或硬编码数据根。
7. 六格消费同一 `.agp` SHA；原生执行与 existence/contract-only 证据等级没有混写。
8. 一次真实 App-only CI run 证明不调用 Cargo、不重编 Base，且合同测试全绿。
9. `lint`、`check --quick` 与所有 owning smoke 在集成树上通过；文档 redaction 无命中。

### 8.B 轨 B（v0.1.17 承接树）

1. **R1e/R2e/R4e** 取得合同所列证据，不能以另一次不同配置 run 或书面猜测替代。
2. **T-debt-linux-package / T-debt-supply-chain** 已由各自 owner 修复，或带 typed
   skip 原因与后续版本去向；不得用宽泛 skip 伪装绿色。
3. **W1–W4** 的干净身份、多窗、multi-client 与独占语义证据全齐。
4. **U2** Windows 与 **O-evidence** macOS 真机证据均齐；缺主机证据时不得标完成。
5. **L7 + L1 + DOC-PRD** 完成，且 DOC-PRD 的对账范围包含 con（23–27）与 cu
   （28–31）两个新子树。
6. **QJS-M6 / E1–E4** 有实现证据或明确可追踪的后续版本决定。
7. **E5** 调用者迁移完成并删除 `agenterm cli script` 的 dispatch/help/catalog。

### 8.C 轨 C（`agenterm-con`）

1. con 自有对齐门与完整套件在精确 unwind profile 下全绿（GC1）。
2. CON-C1 的每一步切分都有"公开合同字节不变"的证据（GC2）；不留半切状态。
3. CON-budget 必须以可复现正式构建证明严格小于 1 MiB（最大 1,048,575 B）；否则
   如实记录超预算数值（GC3），**不得因为超预算就把这条从验收表里拿掉**。
4. CON-C3 完成后，同一增量不再同时出现在 ARCHITECTURE 与 PRD 27/24。

### 8.D 轨 D（`agenterm-cu`）

1. CU-D0 设计稿与 PRD 29–31 逐条对齐，PRD 02 已登记可执行角色（GD1）。
2. CU-D1 机制缺口表完成，每项去向明确（GD2）。
3. CU-D2 与 CU-D3 **同时**通过（GD3）；授权/审计缺失时 `current` 档不得标记任何
   shipped 状态。
4. 全轨无任何 OS API 直调绕过 `agenterm-platform`，无外部 computer-use 依赖进入
   产品图。

### 8.E 轨 E（`libagenterm`）

轨 E 的"完成"只到**形态判决**，不含任何产品迁入：

1. Phase 0 四条判据（独立产物预算 / 共享收益 / 渲染性能 / 行为等价）各自有实测
   数字，**通过与否都记录**——不得因为不利就不记。
2. 判据通过 → §14.7 的 PRD 晋升路径启动；判据不过 → §14 整节删除，§9 决策记录
   留一行否决理由与数字。
3. 无论结果如何，本版**不迁入任何产品**。Phase 1–3 的消费者迁移属于后续版本。
4. 三道边界闸（导出清单 / 头文件同步 / 产品名）的设计已落地为可执行定义，
   即使 Phase 0 否决也保留为机制层的边界纪律输入。

### 8.X 跨轨

1. `lint`、`check --quick` 与所有 owning smoke 在集成树上通过；文档 redaction 无命中。

任一项缺证据则保持 `[ ]`，不得用“设计已定”“可以推断”或交叉编译 existence 代替完成。

---

## 9. 决策记录

| 日期 | 决定 |
|------|------|
| 2026-08-10 | 将原 `plan-agenterm-app-pack.md` 的生效架构、Phase 0 和后续 Phase 去向收敛到本文件；原稿转入 archive，仅保留历史推演价值。 |
| 2026-08-10 | 本版唯一结果是 Portable App Substrate，不把 APE、多架构 loader、WASM、OTA 或真实产品迁移并入 Phase 0。 |
| 2026-08-10 | “跨平台”以同一 `.agp` 字节身份和 App-only 无 Cargo lane 为决定性证据，不宣称单一原生二进制。 |
| 2026-08-10 | QJS App ABI 采用最小 surface，不复制 Rh 全 catalog；Gate 失败不自动回退到目标相关的 Rh AOT App。 |
| 2026-08-12 | **v0.1.17 归档**，其未完成叶整树 upsert 至本文件 §11；本文件成为唯一在制版本计划。原"待 v0.1.17 收口再开工"的外部前置改为本版内部依赖（§0.3），轨 A 仍受其约束，轨 B/C/D 不受阻。 |
| 2026-08-12 | `agenterm-con` 从"尾账"升为**独立产品轨**（§12），有自己的 GC1–GC3 Gate。其绿状态与工作台互不替代；PE 字节归 PRD 27/24，结构债归 ARCHITECTURE §4 C1–C3。 |
| 2026-08-13 | **轨 E 规格修订（§14.3.5）**：初版假设 platform 提供拉取式事件与可跨调用存活的帧指针，实测不成立——`window_host` 只暴露阻塞回调循环 `run_pixel_window(Box<dyn PixelWindowApplication>)`，且 `render()` 的 `XrgbPixelFrame<'_>` 借用被回调作用域锁死。在"不改 platform"约束下按初版规格实现**不可能**；两次外部 agent 派单各烧约 5,000 行轨迹零产出，根因是任务不可完成，非执行方能力问题。**采纳方案 (a)**：库内私有线程跑循环 + begin/commit 会合把控制权交回调用方，零拷贝得以保留。**暂不采纳方案 (b)**（platform 长出 pump/step API）——那是机制层新契约，归 `PRD_02_20`，不该由薄导出壳私自决定；留作 Phase 0 后议题。 |
| 2026-08-12 | **决策项 P4 拍板**：`agenterm-cu` 立项，归 PRD 28–31 专属子树，首发 `current` 档，工作名 `agenterm-remote.exe` 作废。本版只做设计与 `current` 原型（§13），任何 tier 在授权/审计通过前不得标 shipped，含 `current` 档在内不豁免。 |

---

## 10. 开工检查单

1. 先认自己在哪条轨：**A** App Substrate（§0–§10）、**B** 承接树（§11）、
   **C** `agenterm-con`（§12）、**D** `agenterm-cu`（§13）、**E** `libagenterm`（§14）。
   只读本轨叶 + 跨轨非目标（§6），不要把别轨的 Gate 当自己的。
2. 归档稿（`archive/plan-v0.1.17.md`、`archive/plan-agenterm-app-pack.md`）只用于
   追溯，不作为执行依据。
3. 轨 A：先冻结 Host ABI/manifest fixture，再写 loader 或 Engine glue；§0.3 四项
   前置未由轨 B 交付前不开工。
4. 轨 C：动巨石前先跑 con 自有对齐门（GC1）；每步切分要有"公开合同字节不变"的证据。
5. 轨 D：CU-D0 设计稿未与 PRD 29–31 对齐前不写产品代码；任何 OS 机制先看
   `agenterm-platform` 有没有，没有就往那里加，不在 cu 直调。
6. 轨 E：只做 Phase 0 判决切片（pty/process/window+frame/screenshot 四组），
   `agenterm-ui-core` 一律不进库；本版不迁入任何产品。
6. 声明独占 pathspec；根 manifest、公共 schema、workflow 和 Script dispatch 串行修改。
7. cheap lint/check 先于 Cargo；App-only 变更不得借机触发 Base 全矩阵。
8. 小步提交；能力状态变化同步 owning PRD/catalog——con 写 PRD 23–27，cu 写 28–31，
   目录/结构债写 `ARCHITECTURE.md`，**同一事实不要写两处**。
9. 不创建 Candidate/Promotion，除非收到明确 exact-SHA 授权。

---

## 11. 轨 B：承接自 v0.1.17 的收口树

> 来源：[`archive/plan-v0.1.17.md`](archive/plan-v0.1.17.md)，2026-08-12 归档时按
> `plan/README.md` §归档规则 2 整树 upsert。**已完成叶不迁入**（Rh-M23、G-P1/G-P2
> 决策、Common-M7 等保留在归档文件中作为历史事实）。叶定义、不变量、证据与非目标
> 保持原文效力，本节只做归口；不得因为换了文件就重写合同或悄悄降级。

选择原则（继承 v0.1.14/15/16/17）：**宁可少而全绿，不要多而半途**。

### W. 多 GUI / 多窗产品面

- [ ] **W1 重启纪律与版本可观测** — 旧 server/GUI 混跑不得表现为无法判断身份；
  不静默杀会话、不削弱 remain-on-exit/keep-server，版本与实例身份必须从公共 CLI
  可读。身份不一致时明确停止并提示干净重启，非目标是全局 `taskkill` 或自动迁移。
- [ ] **W2 As Window 黑盒（激活标签）** — 必须真开第二窗而非只 focus 原窗；spawn
  带 `--ui-client`，endpoint/instance 选择互斥且可验证。隔离 IPC/workspace 下证明
  GUI 进程数 +1、`ui-lease status.clients` ≥ 2、两窗均可交互。失败保留原窗与原
  lease，非目标是同窗热切换或强制接管。
- [ ] **W3 多 clients 可观测** — status/snapshot 不得继续谎称唯一 GUI；`attached`
  与 `clients[]` 来自同一 lease 权威，稳定 ID 不以标题或索引代替。过期记录显式
  stale/unavailable。
- [ ] **W4 独占语义清扫** — 残留 `exclusive`/`already attached` 文案会让已支持的
  多 lease 路径看似失败；As Window 源码锁仍要求 `--ui-client`，产品路径不得回退为
  "只 focus 不双开"。发现语义不一致则保持 unavailable 并登记 `parity-gap:`。

### U/O. 跨主机证据尾账

- [ ] **U2 Windows 标签切换假刷新回归** — 空 composer 连点 tab 不应制造
  `ComposerDraft` 事件风暴；无草稿变化就无草稿写入。无法取得真机时保持未完成，
  不以单元测试代替。
- [ ] **O-evidence macOS 多实例真机闭环** — picker/open-instance/strip 菜单需用户
  可操作证据；切换 instance、As Window、keep-server 后再附着均指向所选权威。不把
  交叉编译、existence-only 或 Linux X11 结果冒充 macOS 真机证据。

### R′. 发布链证据收口

- [ ] **R1e Candidate worker/cache 连续证据** — 同一配置连续两次 Candidate，第二次
  必须记录 `bootstrap.worker.state==reused` 且配额未驱逐；无第二次 run 时保持未完成，
  不为制造证据重写 cache 策略。
- [ ] **R2e cargo-home restore 前缀证据** — 以 R1e 第二次 run 的 restore 日志证明
  `cargo-home-candidate-v2` 前缀命中；日志缺失或 key 不同则 fail-closed 为未证明。
- [ ] **R4e release rehearsal 无副作用证据** — 真实运行 `release.cmd --rehearse`
  并证明无 tag、draft、Release 或远端写；任一身份/资产错误须停止且保持远端不变。
- [ ] **T-debt-linux-package** — `linux_package` 缺 archive/SBOM/receipt 时给出最小
  复现和确定修复；缺件必须阻止封存。
- [ ] **T-debt-supply-chain** — 计数/catalog pin 漂移以 resolved lock graph 证据
  收口，不一致时输出 typed diff 并失败，非目标是放宽计数或跳过依赖审计。

### G′′. 安装/更新体验尾账

> G-P1/G-P2 已在 v0.1.17 拍板并保留效力：无 signed asset 时自动选 unsigned-preview
> 并强制多行信任警告；保持 server/会话、版本差异必须提示、不自动 kill、一键 apply
> 默认关闭。

- [ ] **G1** — macOS 无 signed asset 时自动选 unsigned-preview 并输出不可静默的
  多行信任警告；缺 preview 或身份不匹配即停止，绝不把 unsigned 包装成 stable。
- [ ] **H2** — install.sh 只从已校验的 `releases.json` 选版本与资产，index 绑定
  sealed manifest SHA、source SHA、tag、version 与六 artifact；失败不回退到猜文件名。
  依赖 H1 至少一轮稳定证据。
- [ ] **G7b 版本不一致提示** — 升级前展示两端版本与继续后果；无法探测时诚实标
  unknown，不假定相容。
- [ ] **G7c 保持 server/会话** — 默认 keep-server 且不自动 kill；失败不得损坏现有会话。
- [ ] **G7d 显式 apply** — 只能由显式 flag 开启且默认 off，覆盖确认、精确目标和
  失败保会话；非目标是后台自动重启。

### L′. 低成本尾账（自 v0.1.14 连续迁入）

> 顺序：L7 → L1 → DOC-PRD → L5 → L6 → L4 → L2/L3。

- [ ] **L7 多文件格式前置纪律** — 多文件 Rust 改动在昂贵编译前跑
  `cargo fmt --check`/仓库 lint；格式失败安全停止，非目标是新增另一套入口。
- [ ] **L1 身份真机回归** — 干净二进制以 custom instance 启动后 `server-list` 必须
  显示用户 scope 标签而非误标 main；失败保留原记录并显式报身份不一致。
- [ ] **DOC-PRD capability 对账** — 把最终 shipped/planned 状态同步到 owning PRD 与
  稳定 catalog；plan/PRD/alignment 三方无漂移，冲突时停在 planned 而不虚报 shipped。
  **本版新增范围**：`agenterm-con` 子树（PRD 23–27）与 `agenterm-cu` 子树
  （PRD 28–31）也在对账范围内。
- [ ] **L5 Control Center smoke 进 CI 评估** — 写明进入/不进入哪个 lane、墙钟预算
  与唯一 owner；不把 GUI 工作藏入声称跳过 smoke 的 lane。
- [ ] **L6 stale 注册记录体验** — `server-list`/cleanup 可识别 stale 但绝不误杀 live
  server；黑盒覆盖 stale 与 live 并存。
- [ ] **L4 Control Center 矮窗 tab 条** — 约 480px 高度仍能折叠/滚动/导航；不借此
  启动 Control Center 大改版。
- [ ] **L2 persistent-worker dedup 上限** — 先记录预算/淘汰决策，再实现有界行为与
  饱和测试；不得把 robustness budget 解释成权限。
- [ ] **L3 无 HOME/XDG 的实例目录** — 决定并验证 fallback 私有目录、符号链接与祖先
  目录完整性；失败必须拒绝不可信共享位置。

### Ux. Windows 尾账余量

- [ ] **U4 generation-aware TabSelected delta**（可砍，但不得缩写成无合同优化）—
  仅 active 变化且 screen generation 一致时省略整屏 cells；generation 落后、未知或
  断档必须 fail-closed 拉全量。
- [ ] **S4 同窗热切换权威（默认 v0.2）** — 若拍板实现，状态机必须是确认 → detach
  当前 lease → 换 endpoint → 新 bootstrap；失败回到原 context 或诚实断开，绝不串
  PTY。本版只保留去向，不把"边界文档"冒充 S4 完成。

### 引擎与测试基建债

- [ ] **QJS-M6 operation catalog 静态对账** — 以 `OPERATION_CATALOG` 为权威：已知
  literal operation 在 `check`/`check-many` 通过，未知 literal 返回 typed
  fail-closed，动态表达式标为不可静态证明且不得虚报通过。`capability` 仅为发现/
  兼容元数据，绝非授权。
- [ ] **E1 qjs pack 身份语义** — 决定字节码 hash 是仅 provenance 指纹还是加载权威
  并写入公开 contract；不能把未消费 hash 描述成可执行字节绑定。
- [ ] **E2 lua fail-closed entry** — 明确并测试缺 entry/坏 entry 的稳定错误与退出码；
  不要求 lua 复制 rh AOT 机制。
- [ ] **E3 rh shipped surfaces 对账** — 对 32 条 host catalog 缺失声明逐项删除、实现
  或标 planned/unavailable；未实现 API 是产品缺口，不是权限裁剪理由。
- [ ] **E4 测试孤儿进程** — owning tests 必须清理其 `agenterm server` 子树并证明构建
  输出可覆盖；安全失败保留诊断但不做宽泛进程清理。
- [ ] **E5 删除已弃用的 `agenterm cli script` 入口** — 先迁移仓库全部调用者，再由
  CLI dispatch/help/catalog 与跨引擎黑盒证明四个按引擎入口仍可发现和执行；旧入口
  返回稳定 typed unknown-command 失败。任何仍有 owner 的调用者未迁完时不得先删
  dispatch。

### 跨版轨态度（不变）

| 轨 | 本版态度 |
|----|----------|
| **M** 多 agent 观察 | 文档/约定可补；大功能仍推 v0.2.x |
| **N1** platform facade | 可选小叶；不阻塞其他 |
| **L-CC** | 设计稿已有；实现默认 **v0.2.0** |
| **L-NET** | 研究继续，**不进**本版 must-ship |
| **L-CU** | **本版态度变更**：已立项，见 §13 |

---

## 12. 轨 C：`agenterm-con` 第二产品

> 产品真理：PRD 子树 [23](../prd/PRD_02_23_agenterm_con.md) 根 +
> [24](../prd/PRD_02_24_con_terminal.md) 终端渲染 /
> [25](../prd/PRD_02_25_con_workspace.md) 工作区输入 /
> [26](../prd/PRD_02_26_con_control_cli.md) 控制与 CLI /
> [27](../prd/PRD_02_27_con_delivery.md) package 与交付。
> 结构债：[`ARCHITECTURE.md`](ARCHITECTURE.md) §4 C1–C3。

con 已是独立 package、有独立 CI 与独立对齐门，不再是"主程序的一个 bin"。本版
**不把 con 当作尾账**，而是当作一条有自己 Gate 的产品轨。

### 不变量（本轨全程）

- con 的绿状态不得来自工作台测试，工作台的也不得来自 con；两条 CI 在同一 SHA
  各自成功才进 Candidate。
- 体积陈述必须指明 profile。不得以恢复 abort、削弱背压/durability/清洁关闭换体积。
- PE 字节、perf 探针与证据计数只写进 PRD 27/24；目录、mod 与巨石切分只写进
  ARCHITECTURE。见 ARCHITECTURE §4 C3。

### 叶

- [x] **CON-budget 严格 <1 MiB** — 精确 custom-std unwind/trace-only
  正式产物最大为 1,048,575 B；旧 512 KiB 目标已于 2026-08-12 被此上限取代，历史
  超额数字保留为优化证据而不再构成交付失败。2026-08-12 官方 `con-release`
  custom-std unwind/trace-only 产物为 560,128 B，低于上限 488,447 B；证据来自
  target-specific cold build，不是增量构建数字。
  安全失败：拿不到可复现证据就保持未完成并如实记录，不改预算定义蒙混过关。
  非目标：回退 unwind、砍掉已验收的 resize/close 语义。
- [ ] **CON-C1 主体巨石切分** — `src/bin/agenterm-con.rs` 6,238 行占 con 产品源码
  60%，VT 回调、终端状态机、`ConApp`、perf 计数、待决控制请求与像素 `Surface`
  同居。按 PRD 24/25/26 既有边界切分；**先切 `ConApp` 的待决请求与 perf 状态**
  （已有 PRD 26 契约兜底），再动渲染路径。不变量：切分不得改变任何公开 CLI/JSON
  合同字节；每步由 con 自有黑盒与对齐门证明无行为变化。非目标：借切分改产品语义。
- [ ] **CON-C2 package 物理分离** — 源码与测试迁入 `crates/agenterm-con/`，消除
  `[[bin]]`/`[[test]]` 的 `../../` 回指。迁移前 PRD 27 的物理分离条目保持 `[~]`，
  不得因依赖图已独立就宣称布局已分离。非目标：顺手重排工作台目录。
- [ ] **CON-C3 文档双写止血** — ARCHITECTURE 第 236–562 行的 con 体积史/证据计数
  与 PRD 27/24 平行记录，且曾领先 PRD 两代。按 ARCHITECTURE §4 C3 的单主规则做一次
  去重扫描：结构规则留 ARCHITECTURE，字节与证据归 PRD。证据是两文档对同一增量不再
  各记一份。非目标：删除历史数字。
- [ ] **CON-residual 行为余量**（承接 v0.1.17 C 组）— 真实 TUI 方向键与备用屏滚轮、
  IME 端到端人工验收、脚本化拖拽黑盒。缺主机证据的项保持 `[ ]`，不以单测代替。
- [ ] **CON-font 字体度量**（承接）— 采用与主程序同一原生字体/格宽度量路径，验证
  ASCII 1 格、CJK/宽字符 2 格。非目标：在 custom rasterizer 中混拼 font face。
- [ ] **CON-C10d 可选三叶**（承接，不阻塞 must-ship）— 回看搜索、OSC 8 链接、脏行
  重绘各自独立验收。非目标：server attach 或完整 conhost 替代。

### 轨 C Gate

| Gate | 必须证明 | 不通过时 |
|------|----------|----------|
| **GC1** | con 自有对齐门 + 完整 con 套件在精确 unwind profile 下全绿 | 不动巨石 |
| **GC2** | CON-C1 每一步切分后公开合同字节不变 | 回滚该步，不累积半切状态 |
| **GC3** | CON-budget 有可复现的 linked-symbol/disassembly 证据 | 如实记录超预算，不宣称达标 |

---

## 13. 轨 D：`agenterm-cu` computer-use 立项

> 产品真理：PRD 子树 [28](../prd/PRD_02_28_agenterm_cu.md) 根 +
> [29](../prd/PRD_02_29_cu_command_surface.md) 命令面 /
> [30](../prd/PRD_02_30_cu_targets_transports.md) 目标与传输 /
> [31](../prd/PRD_02_31_cu_authorization_safety.md) 授权与审计。
> 设计输入：[`plan-v0.1.15.md`](plan-v0.1.15.md) §5.6 主线 L-CU、
> [`agent-human-parity-audit.md`](agent-human-parity-audit.md)（`current` 档现状）。

**决策项 P4（是否立项 / 归口哪个 PRD / 首发平台）已拍板**：立项，归 PRD 28–31
专属子树，首发 `current` 档。本轨在 v0.1.18 内**只做设计与 `current` 档原型**，
不承诺任何 tier 进入 shipped。

### 不变量（本轨全程）

- 不重造第五套截图/输入实现。OS 机制一律经 `agenterm-platform`，缺什么就往 platform
  加 typed `Available/Unsupported/Failed`，不在 cu 里直调 OS API。
- `current` 是协议族的 local 退化档，不是临时方案。加 transport 只换传输层，不动
  上层命令集。
- 不继承脚本引擎那套"无限制本地运行时"姿态。**含 `current` 档在内**，任何 tier 在
  其授权/审计/拒绝语义通过前不得标记 shipped。
- 参考实现（隔壁 monorepo `skills/computer-use/`）只是设计输入，代码/运行时/依赖
  一律不进产品图。

### 叶

- [ ] **CU-D0 设计稿落地** — 产出 `plan/design-agenterm-cu.md`：抽象命令集清单、
  洋葱分层的具体 mod 边界、可执行形态与进程模型、`current` 档后端选型。用户问题是
  没有设计稿就会直接长成第五套输入实现。证据是设计稿逐条对上 PRD 29/30/31 的条目，
  且在 PRD 02 登记可执行角色。安全失败：形态未定前不写产品代码。
- [ ] **CU-D1 platform 机制缺口测绘** — 对照命令集，列出 `agenterm-platform` 已有
  与缺失的机制（控件树枚举是最大缺口）。证据是一张缺口表 + 每项的 typed 契约草案。
  非目标：本叶不实现机制。
- [ ] **CU-D2 `current` 档原型** — 在一个平台上打通"截图 + 枚举窗口 + 枚举控件树 +
  点击 + 输入"最小竖线。不变量：结构化身份优先于像素，退化为坐标必须在结果里显式
  可见。证据是公开黑盒对真实目标跑通并等待状态而非 sleep。安全失败：控件树不可用
  时返回 typed 退化模式，不静默猜坐标。
- [ ] **CU-D3 授权与审计骨架** — 与 CU-D2 同步落地，不得后补。证据是黑盒证明未授权
  动作被拒、撤销后不再生效、已授权动作有记录、凭据不出现在任何已发布产物中。
  安全失败：审计路径不可用则动作不执行。
- [ ] **CU-D4 agenterm 作为 cu 目标** — 把 `ui-snapshot` 的精确 bounds 接成 cu 的
  结构化观察源，验证"agenterm 是第一个自带结构化控件树的 computer-use 目标"这一
  差异点。非目标：本版不做远程 tier。

### 轨 D Gate

| Gate | 必须证明 | 不通过时 |
|------|----------|----------|
| **GD1** | CU-D0 设计稿与 PRD 29–31 逐条对齐，PRD 02 已登记可执行角色 | 不写产品代码 |
| **GD2** | CU-D1 缺口表完成，机制去向明确（进 platform 还是不做） | 不在 cu 内直调 OS API |
| **GD3** | CU-D2 + CU-D3 同时通过 | `current` 档不得标记任何 shipped 状态 |

**轨 D 非目标（本版）**：ssh/rdp/vnc 任一远程 tier 的实现、模型/规划/agent loop、
引入外部 computer-use 框架、公开发布 cu 可执行体。

---


---

## 14. 轨 E：`libagenterm.{so,dylib,dll}` 机制库（已接受规划，未开工）

> 本节自 [`archive/plan-libagenterm.md`](archive/plan-libagenterm.md) 全文合并（2026-08-12），
> 原文件已按归档规则移入 `archive/` 仅保留追溯价值。本节是该方向的**唯一执行投影**：
> 已接受规划、未开工；Phase 0 四条判据出结果前，不得把 libagenterm 写成 PRD 已接受范围。

目标消费者：`agenterm`、`agenterm-con`、`agenterm-cu`。
关联：[`ARCHITECTURE.md`](ARCHITECTURE.md) §1.0、本文 §1（轨 A 的 Host ABI，**另一条轴**）、
[`plan-ape-thin-shell-dynamic-packages.md`](plan-ape-thin-shell-dynamic-packages.md)

---

### 14.1 为什么做（和为什么不做）

| 动机 | 判定 |
|------|------|
| 省体积 | **待实测**。共享库可能消除三个消费者的重复机制，也会引入 feature 并集；每个库产物必须独立严格小于 1 MiB |
| 省构建时间 | **不成立**。ape 计划已测明靶心是 agenterm 那个 165 文件 monolith |
| **跨语言消费** | **成立**。cu 的后端参考是 Swift AX / Python UIA；wbox 等 embedding 同理 |
| **机制独立更新** | **成立**。改一个 ConPTY bug 不必重编六格三个二进制 |
| **边界机器可检** | **成立**。导出符号表比 prose 纪律强 |

立项主因是后三条；减少重复字节是需要 Phase 0/1 证明的收益，不是预支结论。

---

### 14.2 两条硬规则

**规则一：函数体是 syscall 才能过 ABI，是纯算术就必须静态链接。**

| 进库 | 留静态 rlib |
|------|------------|
| `pty` `process*` `ipc` `filesystem*` `clipboard` `screenshot` `window*` `ime` `input_inject` `shared_memory` `runtime` `font`(已缓存) | **整个 `agenterm-ui-core`**；`numeric` `byte_search` `checksum` |

ui-core 是逐行热路径。PRD 24 记的 895→360 us 与零 host copy 全建立在直接 raster
进 retained XRGB buffer 上；每行穿一次 FFI 会把它吐回去。

**规则二：编译期 feature → 运行期能力查询。** dll 无法为每个消费者裁剪。
好在这与现有 `Available/Unsupported/Failed` 三态同构，只是把"这构建没编进"
并入同一通道。代价记账：con 的最小依赖图纪律从"链接期不含"退化为"运行期不调用"。
因此动态库本身必须 `<= 1,048,575 B`，迁移后的 con EXE 也必须独立满足同一上限。

---

### 14.3 接口

`crate-type = ["cdylib"]`，C ABI。新增 `crates/agenterm-abi/` 薄导出壳（~2–3k 行），
`agenterm-platform` 那 47k 行原样不动。符号前缀 `agt_`，不含任何产品概念
（tab / workspace / Fleet / lease / instance）。

#### 3.1 版本与错误

```c
uint32_t    agt_abi_version(void);   /* (major<<16)|minor；major 不符拒绝加载 */
const char* agt_build_id(void);      /* 语义版本 + 源 SHA */

typedef enum { AGT_OK=0, AGT_UNSUPPORTED=1, AGT_FAILED=2 } agt_status;

typedef struct {
  const char* operation;  /* 静态，永久有效 */
  const char* code;       /* 静态 */
  const char* message;    /* 线程局部，有效至本线程下次调用 */
} agt_error;
agt_status agt_last_error(agt_error* out);
```

现有 `PtyError` 已是 `Unsupported{operation,reason}` / `Failed{operation,code,message}`，
且 `operation`/`code` 本就是 `&'static str` —— 零分配即可暴露为稳定 C 字符串。
**不变量：这两态永不合并**（否则上层分不清"平台没有"和"这次没成"）。

#### 3.2 能力协商

```c
typedef enum {
  AGT_CAP_PTY=1, AGT_CAP_PROCESS_SPAWN, AGT_CAP_PROCESS_OBSERVE,
  AGT_CAP_WINDOW_HOST, AGT_CAP_WINDOW_ENUMERATE, AGT_CAP_WINDOW_OP,
  AGT_CAP_SCREENSHOT, AGT_CAP_CLIPBOARD, AGT_CAP_IME, AGT_CAP_INPUT_INJECT,
  AGT_CAP_IPC, AGT_CAP_FONT_RASTER, AGT_CAP_FILESYSTEM_PUBLISH,
  AGT_CAP_SHARED_MEMORY, AGT_CAP_PARENT_CONSOLE
} agt_capability;

agt_status agt_capability_query(agt_capability);  /* 只返回 OK 或 UNSUPPORTED */
```

#### 3.3 句柄与线程亲和

```c
typedef struct agt_pty*     agt_pty_t;
typedef struct agt_window*  agt_window_t;
typedef struct agt_process* agt_process_t;
typedef struct agt_frame*   agt_frame_t;
```

不透明，库拥有，显式 `_close`。**亲和性必须写进头文件**：

- `agt_window_t` / `agt_frame_t` —— **创建线程专属**（即调用 `agt_window_open`
  的那个线程）。注意这**不是**平台事件循环线程：循环由库自己起的私有线程承载，
  见 §3.5 的控制反转会合。
- 字形 raster —— 创建线程专属（`CreateCompatibleDC(NULL)` 的 HDC/HFONT 规则）
- `agt_pty_t` / `agt_process_t` —— 跨线程安全

#### 3.4 缓冲区：调用方分配，两段式

```c
/* cap 不足 → FAILED + code="buffer_too_small"，所需长度写进 out_len */
agt_status agt_pty_read(agt_pty_t, uint8_t* buf, size_t cap, size_t* out_len);
```

库**从不**把内存所有权交给调用方。彻底消灭"谁 free"。

#### 3.5 事件与帧：控制反转会合（修订 2026-08-13）

> **为什么改**：初版规格假设 platform 提供拉取式事件与可跨调用存活的帧指针。
> 实测不成立——`agenterm-platform::window_host` 只暴露
> `run_pixel_window(options, Box<dyn PixelWindowApplication>)`，是**阻塞式回调
> 循环**，且 `PixelWindowApplication::render(&mut self, _, frame: &mut
> XrgbPixelFrame<'_>)` 的帧借用被 `render()` 作用域锁死。按初版规格实现，在不改
> platform 的前提下**不可能**。两次外部 agent 派单各烧掉约 5,000 行轨迹零产出，
> 根因就是这条不可完成的要求，不是执行方能力问题。

**采纳方案 (a)：库内起私有线程跑事件循环，用会合把控制权交回调用方。**
不改 `agenterm-platform`（方案 (b) 让 platform 长出 pump/step API 更干净，但那是
机制层新契约，归 `PRD_02_20`，不该由"薄导出壳"私自决定；留作 Phase 0 后议题）。

模型：

```text
调用方线程                          库私有循环线程
-----------                        ----------------
agt_window_open  ──起线程──▶        run_pixel_window(...)
                                     ├─ event()  → 事件入有界队列
agt_window_poll_event ◀──队列──┘     └─ render() → 停在此处等会合
agt_frame_begin  ──会合──▶            （借用移交，回调保持不返回）
  写 pixels（静态 ui-core）
agt_frame_commit ──放行──▶            回调带 directive 返回
```

```c
typedef enum { AGT_EV_NONE=0, AGT_EV_GEOMETRY, AGT_EV_POINTER, AGT_EV_WHEEL,
               AGT_EV_KEY, AGT_EV_IME, AGT_EV_FOCUS, AGT_EV_EXPOSE,
               AGT_EV_RENDER_DUE, AGT_EV_CLOSE_REQUEST } agt_event_kind;

typedef struct { uint32_t kind; uint64_t generation; union { /* POD */ } data; } agt_event;

/* 事件来自库内有界队列，由循环线程的 event() 回调填入。队列满按既有有界策略
   fail-closed，不得无界增长。 */
agt_status agt_window_poll_event(agt_window_t, agt_event* out, uint32_t timeout_ms);

typedef struct {
  uint32_t* pixels;  /* 借出，仅在 begin↔commit 之间有效 */
  uint32_t  width, height, stride_px;
  uint64_t  generation;
  uint32_t  retention;   /* retained | transient */
} agt_frame_info;

/* 阻塞直到循环线程进入 render()；超时返回 FAILED + code="render_not_due"。
   收到 AGT_EV_RENDER_DUE 后调用可立即成功。 */
agt_status agt_frame_begin  (agt_window_t, agt_frame_t* out, agt_frame_info*,
                             uint32_t timeout_ms);
agt_status agt_frame_commit (agt_frame_t, const agt_pixel_rect* damage, size_t n);
agt_status agt_frame_abandon(agt_frame_t);
```

`n==0` → Full，`n>0` → bounded partial，`abandon` → None，与现有 platform 合同
一一对应。调用方用**静态链接的 ui-core** 直接写 `pixels`——零拷贝因此保住。

**会合不变量（全部可测）**：

- `pixels` 仅在 `begin` 成功返回后至 `commit`/`abandon` 之间有效；之后立即失效，
  debug 构建须毒化并在二次使用时 fail-closed。
- 同一 `agt_window_t` 同时只允许一个未 commit 的帧；重入 `begin` 返回
  `FAILED + code="frame_in_flight"`。
- 调用方在 `begin` 与 `commit` 之间**不得**调用任何其他 `agt_window_*`——循环线程
  正停在回调里，会死锁；库须检出并返回 `FAILED + code="reentrant_during_frame"`。
- 调用方未 commit 就 `agt_window_close`：库必须 `abandon` 该帧、放行回调、有界回收
  线程，不得让循环线程永久阻塞。
- 回调仍在库内 `catch_unwind`；panic 不得穿越 FFI，也不得让会合方永久等待。

**代价要记账**：多一条线程 + 一个有界事件队列 + 每帧一次会合往返。
Phase 0 判据里的渲染四项（frames / full-candidate / dirty-pixel / native-present）
就是用来量这个代价的；`host_copy_frames` 必须仍为 0，否则零拷贝已被吃掉。

#### 3.7 PTY（其余模块同构）

```c
typedef struct { const char* program; const char* const* argv; size_t argc;
                 const char* cwd; const char* const* envp; size_t envc;
                 uint16_t cols, rows; } agt_pty_spawn;

agt_status agt_pty_open  (const agt_pty_spawn*, agt_pty_t* out);
agt_status agt_pty_write (agt_pty_t, const uint8_t*, size_t, size_t* written);
agt_status agt_pty_resize(agt_pty_t, uint16_t cols, uint16_t rows);
agt_status agt_pty_wait  (agt_pty_t, uint32_t timeout_ms, int32_t* exit_code);
void       agt_pty_close (agt_pty_t);
```

#### 3.8 panic 围栏

每个导出函数 `catch_unwind` 兜底，转 `AGT_FAILED{code="panic"}`。
库必须以 `panic = "unwind"` 构建——与 con 的 `con-*` profile 同源理由。

**实现约束（2026-08-13 里程碑 1 实测）**：Cargo 不允许在 package 级 profile 覆盖
`panic`，故本库沿用 con 的做法新增 `abi-dev` / `abi-release` 两条 unwind profile。
后果必须写进头文件与门禁：**默认 `cargo build/test -p agenterm-abi` 继承 abort
profile，此时 `catch_unwind` 不生效**；panic 围栏只在 `--profile abi-dev|abi-release`
下成立。交付与门禁一律用后者，不得拿默认 profile 的绿色冒充围栏已验证——con 早期
正是在这里破坏过合同（见 [27](../prd/PRD_02_27_con_delivery.md)）。

**边界闸补强（同日实测）**：里程碑 1 的导出测试解析 `src/lib.rs` 源码文本，
不是查产物，达不到 §14.5 第 1 条"实际导出符号集 == 清单"的要求。独立复验用
`llvm-nm --defined-only target/<profile>/agenterm_abi.dll.lib` 并过滤 MSVC 的
`__imp_` 导入 thunk，结果与 `exports.txt` 一致。**该产物级比对必须固化进测试**，
否则边界闸对 `#[export_name]`、依赖再导出等旁路是瞎的。

---

### 14.4 与 App Host ABI v1 的关系

```text
App guest (QJS .agp) ──Host ABI v1──▶ 产品层 ──libagenterm ABI──▶ OS
                        产品语义边界              机制边界
```

两条轴不同，**永不合并**。硬规则：**App guest 不得看到 `agt_*` 符号**，
否则 App 能绕过产品 authority 直接操作 OS。该规则进 `boundary_tests`。

---

### 14.5 边界闸（三道）

1. **导出清单** —— `crates/agenterm-abi/exports.txt` 是唯一真相，生成 Windows `.def` /
   ELF version script / Mach-O `-exported_symbols_list`；实际符号集多一个少一个都红。
2. **头文件同步** —— `include/agenterm.h` 与实现比对，防漂移。
3. **产品名闸** —— 扩展现有测试：导出符号必须 `agt_` 前缀且不含产品概念词。

---

### 14.6 Phase 0：先出形态判决

只导出 `pty` + `process` + `window/frame` + `screenshot` 四组，
拿 `agenterm-con` 做**并行验证消费者**（保留现有静态版，另建 dylib 变体）。

| 判据 | 阈值 |
|------|------|
| 独立产物预算 | 每个平台 `libagenterm.{dll,so,dylib}` 和迁移后的 con EXE 各自 `<= 1,048,575 B`；不得用合并安装载荷均摊超标 |
| 共享收益 | 三个消费者迁移前后密封总字节实测；Phase 0 可暂时变大，但须给出盈亏平衡点，不得虚报节省 |
| 渲染性能 | 16-step resize journey 的 frame / full-candidate / dirty-pixel / native-present 四项，与静态版差异 **< 5%** |
| 行为等价 | 90 单测 + 21 GUI 黑盒 + 多标签控制旅程全绿；公开 CLI/JSON 合同字节不变 |

四条全过才进 Phase 1。

- **Phase 1**：`agenterm-cu` 首个真实消费者（跨语言理由的来源，无历史包袱）
- **Phase 2**：`agenterm-con` 迁入；EXE/库各自 sub-1-MiB，启动、PTY、渲染和清理不得回退
- **Phase 3**：`agenterm` 迁入并删除 ABI 已稳定承载的重复机制；server、脚本、mux、MCP
  和工作台产品语义留在产品层
- `agenterm-cc` 不在当前承诺消费者集合，后续只有明确需求与实测收益才评估

判据不过 → 本节整体删除并在 §9 决策记录留一行否决理由与实测数字，不留残叶。

---

### 14.7 PRD 归属：规划已接受，Phase 0 后进入

现在没有 PRD 条目是有意的：共享机制方向和三个目标消费者已经接受，但 ABI、迁移和
产物尚未实现，不能虚报 shipped。Phase 0 判定具体动态库形态，而不是重新决定是否复用。

- 判据通过 → 开第三个 PRD 子树（编号自 32 起），拥有机制边界、ABI 稳定性承诺、
  能力协商模型与密封/SBOM 归属；`PRD_02_02` 登记 `.dll` 交付角色；`PRD_02_20` 记一条
  引用（**机制契约仍归 20，ABI 稳定性归新子树**——两回事）。
- 判据不过 → 归档。

在此之前任何 PRD 模块都不得把 libagenterm 写成已接受范围。

---

### 14.8 非目标

- 不把 `agenterm-ui-core` 放进动态库。
- 不用 Rust ABI / `crate-type=["dylib"]`（绑编译器版本、无稳定性、要单独带 libstd）。
- 不做插件系统——本库是**导出**边界，不是**注入**边界。
- 不把"必然减小总体积"当预设；独立 sub-1-MiB 预算和重复字节实测仍是验收指标。
- ABI 里不出现任何产品概念；App guest 不得触达 `agt_*`。
- `.dll` 是第七个需密封与 SBOM 记账的产物，不是免检品。

---

*执行投影，非产品宪法。能力状态以 PRD 为准；本版 App Pack 架构、Phase 0 与后续去向*
*已在本文件收敛，归档讨论稿不得重新作为活跃 SSOT。v0.1.17 的未完成叶已 upsert 至*
*§11，`plan-libagenterm.md` 已全文合并至 §14，两份归档文件只保留追溯价值。*
