# AgenTerm v0.1.18 公开计划（草案）

状态：**草案，待 v0.1.17 收口并由用户授权开工**（2026-08-10）
不创建 tag / Candidate / Release，不触发公开更新，除非人工明确授权。
本文件是版本执行投影，不替代 PRD、结构 SSOT 或 App Pack 详细设计。

**主题：Portable App Substrate——稳定 App Host ABI + 一份 QJS App Pack 跨六格消费。**

本版只证明动态应用底座成立：同一份密封 `.agp` 能被现有六个 OS/ISA Base
携带、校验、加载和重载；修改 App 内容不要求重新编译六份 Base。首个真实产品语义
迁移、远程更新、WASM 计算扩展和 APE/多架构 loader 均不在本版实现范围。

> 结构 SSOT：[`ARCHITECTURE.md`](ARCHITECTURE.md)。
> 上版收口树：[`plan-v0.1.17.md`](plan-v0.1.17.md)。
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

### 0.3 前置条件

- v0.1.17 已冻结最终 capability/catalog 状态，QJS-M6、E3、E4、E5 不再处于漂移中。
- v0.1.17 的 exact-SHA CI 与发布链剩余问题已有最终结论；本版不同时认领其活跃红。
- `agenterm cli script` 已删除；公开引擎入口为 `agenterm rh|lua|qjs|sql`。
- 测试不会遗留锁住构建产物的 `agenterm server` 孤儿进程。

任一前置不满足时，本版保持“未开工”，不得用 App Pack 工作绕过 Base 的不可信状态。

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

## 2. 依赖树

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

## 3. 可执行工作树

每个叶均包含：用户问题、不变量、证据、安全失败、黑盒 owner 与非目标。

### P0. 基线冻结

- [ ] **P0 v0.1.17 最终状态快照**
  - **用户问题**：动态底座不能建立在仍漂移的 Base、catalog 或 CI 红之上。
  - **不变量**：只消费 v0.1.17 最终已证明状态；未完成项保留原 owner 和版本去向。
  - **证据 / owner**：v0.1.17 验收表、exact-SHA CI 结论和 PRD capability 对账共同拥有。
  - **安全失败**：任一前置仍活跃则停止 v0.1.18 产品代码工作。
  - **非目标**：不在本版重做 v0.1.16/17 发布链或 GUI 尾账。

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

未授权公开发布时，**开发完成** = 下列同时成立：

1. P0 前置快照冻结，v0.1.17 活跃红未被双重排期到本版。
2. Host ABI v1、manifest 和稳定 snapshot schema 已进入 owning PRD/catalog，fixture 全绿。
3. QJS 宿主六格可构建，体积、冷/热墙钟、notice 和发布预算有实际证据。
4. `.agp` 确定性构建、hash/provenance、篡改与路径逃逸测试全绿。
5. status/doctor/reload/factory-reset 通过公共 CLI；失败不杀 PTY/server/lease。
6. platform boundary test 证明 App Pack 代码没有平台 cfg、原生 marker 或硬编码数据根。
7. 六格消费同一 `.agp` SHA；原生执行与 existence/contract-only 证据等级没有混写。
8. 一次真实 App-only CI run 证明不调用 Cargo、不重编 Base，且合同测试全绿。
9. `lint`、`check --quick` 与所有 owning smoke 在集成树上通过；文档 redaction 无命中。

任一项缺证据则保持 `[ ]`，不得用“设计已定”“可以推断”或交叉编译 existence 代替完成。

---

## 9. 决策记录

| 日期 | 决定 |
|------|------|
| 2026-08-10 | 将原 `plan-agenterm-app-pack.md` 的生效架构、Phase 0 和后续 Phase 去向收敛到本文件；原稿转入 archive，仅保留历史推演价值。 |
| 2026-08-10 | 本版唯一结果是 Portable App Substrate，不把 APE、多架构 loader、WASM、OTA 或真实产品迁移并入 Phase 0。 |
| 2026-08-10 | “跨平台”以同一 `.agp` 字节身份和 App-only 无 Cargo lane 为决定性证据，不宣称单一原生二进制。 |
| 2026-08-10 | QJS App ABI 采用最小 surface，不复制 Rh 全 catalog；Gate 失败不自动回退到目标相关的 Rh AOT App。 |

---

## 10. 开工检查单

1. 确认 v0.1.17 已完成或明确冻结最终未完成去向。
2. 读取本文件 §1–§6；原 App Pack 归档稿只用于追溯，不作为执行依据。
3. 先冻结 Host ABI/manifest fixture，再写 loader 或 Engine glue。
4. 声明独占 pathspec；根 manifest、公共 schema、workflow 和 Script dispatch 串行修改。
5. cheap lint/check 先于 Cargo；App-only 变更不得借机触发 Base 全矩阵。
6. 小步提交；能力状态变化同步 owning PRD/catalog。
7. 不创建 Candidate/Promotion，除非收到明确 exact-SHA 授权。

---

*执行投影，非产品宪法。能力状态以 PRD 为准；本版 App Pack 架构、Phase 0 与后续去向*
*已在本文件收敛，归档讨论稿不得重新作为活跃 SSOT。*
