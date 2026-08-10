# AgenTerm v0.1.18 公开计划（草案）

状态：**草案，待 v0.1.17 收口并由用户授权开工**（2026-08-10）  
不创建 tag / Candidate / Release，不触发公开更新，除非人工明确授权。  
本文件是版本执行投影，不替代 PRD、结构 SSOT 或 App Pack 详细设计。

**主题：Portable App Substrate——稳定 App Host ABI + 一份 QJS App Pack 跨六格消费。**

本版只证明动态应用底座成立：同一份密封 `.agp` 能被现有六个 OS/ISA Base
携带、校验、加载和重载；修改 App 内容不要求重新编译六份 Base。首个真实产品语义
迁移、远程更新、WASM 计算扩展和 APE/多架构 loader 均不在本版实现范围。

> App Pack 架构与完整分期 SSOT：[`plan-agenterm-app-pack.md`](plan-agenterm-app-pack.md)。  
> 结构 SSOT：[`ARCHITECTURE.md`](ARCHITECTURE.md)。  
> 上版收口树：[`plan-v0.1.17.md`](plan-v0.1.17.md)。  
> 跨目标机制研究：[`reference-cross-target-execution.md`](reference-cross-target-execution.md)。

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

## 1. 依赖树

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

## 2. 可执行工作树

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
  - **不变量**：v1 只暴露 `runtime.*`、只读 `product.*` 占位回调和必要的结构化诊断；
    不暴露 OS handle、平台 cfg、Fleet 状态副本或逐帧渲染入口。`capability` 只表示发现与
    兼容元数据，不表示授权、拒绝或 sandbox。
  - **证据 / owner**：版本化 ABI catalog 与 QJS literal checker 一一对应；已知调用通过，
    未知 literal typed fail-closed，动态表达式诚实标为不可静态证明。
  - **安全失败**：缺 surface 时回到最小安全态并报告精确 operation ID。
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
  - **安全失败**：任一格不能构建或超出既有发布预算时停止 Phase 0，先选择修工具链、
    独立 Runtime Component 或调整范围。
  - **非目标**：不得自动回退到 Rh App Pack；其目标相关 AOT 产物不满足“一包六格”。

- [ ] **Q0b 长驻 QJS Runtime/Context**
  - **用户问题**：当前 run-to-exit 求值不能支持 App 生命周期和不终止 PTY 的 reload。
  - **不变量**：server 进程一份 Engine；回调有预算、取消和 interrupt；中断后 Engine
    标脏并整体重建，不继续使用可能半更新的状态。
  - **证据 / owner**：QJS embed 黑盒覆盖 load、具名 export、重复调用、死循环 interrupt、
    dirty reload 与旧 Engine 资源释放。
  - **安全失败**：失败进入最小安全态；不退出 server，不关闭 tab，不破坏 lease。
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
  - **证据 / owner**：首次解包、factory 升级、user 不覆盖、损坏状态和 factory-reset 黑盒。
  - **安全失败**：无法判定来源时不覆盖现有目录，doctor 给出恢复动作。
  - **非目标**：不下载远程 pack，不自动把本地编辑标成可信远程更新。

- [ ] **L0b status / doctor / reload / factory-reset**
  - **用户问题**：用户需要从公共入口判断正在运行哪份 App 以及如何恢复。
  - **不变量**：所有操作经 `agenterm cli app-pack ...`；路径由 platform policy 返回；
    reload 原子切换，失败保留上一份已知良好 Engine/pack。
  - **证据 / owner**：隔离 instance 的 CLI 黑盒覆盖每个命令、退出码和 snapshot 变化。
  - **安全失败**：reload 失败不终止 PTY/server/lease；factory-reset 不删除非本功能拥有的文件。
  - **非目标**：不增加独立 App Pack PE，不恢复 `agenterm cli script`。

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

## 3. Gate 与执行顺序

| Gate | 必须证明 | 不通过时 |
|------|----------|----------|
| **G0 Base ready** | P0 全部前置已冻结 | 不开工 |
| **G1 ABI frozen** | H1 manifest/surface/state schema fixture 全绿 | 不写 loader |
| **G2 QJS adoption** | Q0a 六格构建、预算、notice、墙钟有实数 | 停止并重新选择宿主形态；不回退 Rh App |
| **G3 minimal load** | Q0b/c + A0 + L0 本地黑盒全绿 | 不建立 App-only lane |
| **G4 portability** | X0a 同一 SHA 六格证据诚实齐备 | 不宣称“一包六格” |
| **G5 decoupling** | X0b 真实 App-only run 不调用 Cargo | 不把 v0.1.18 标为完成 |

严格顺序：`G0 → G1 → G2 → G3 → G4 → G5`。G1 冻结后，QJS Engine 与
`.agp` builder 可并行；公共 schema、根 manifest、workflow 和 Script dispatch 属于集成热区，
由主线串行修改。最终 lint、Quick、Base matrix 与 App-only lane 在同一集成状态上串行验收。

---

## 4. CI 与证据分层

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

## 5. 明确非目标

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

## 6. 后续版本接口

| 后续 | 建议范围 | 本版必须留下的稳定接口 |
|------|----------|--------------------------|
| **v0.1.19** | Phase 1：一条真实 CC 静态语义竖线 | typed callback、fallback、interrupt、event |
| **v0.1.20** | Phase 2/3 取舍：更多静态语义或签名更新 | Host ABI version、origin/state、atomic reload |
| **v0.1.20+** | WASM 计算扩展实验 | 与 QJS 正交的 guest ABI；不得接管 product authority |
| **v0.2.x** | 多架构薄壳/安装 loader 评估 | Base/App 分轨与单一 `.agp` 身份 |

APE 只能作为未来交付封装机制候选重新评估，不能替代 Host ABI、macOS 签名、Windows PE、
ISA 机器码或平台 adapter。WASM 首选定位是 App Pack 的可选计算模块；QJS 负责高频产品语义，
Rh 负责 Build/CI，Rust/Base 负责权威状态与原生机制。

---

## 7. 验收总门

未授权公开发布时，**开发完成** = 下列同时成立：

1. P0 前置快照冻结，v0.1.17 活跃红未被双重排期到本版。
2. Host ABI v1、manifest 和稳定 snapshot schema 已进入 owning PRD/catalog，fixture 全绿。
3. QJS 宿主六格可构建，体积、冷/热墙钟、notice 和发布预算有实际证据。
4. `.agp` 确定性构建、hash/provenance、篡改与路径逃逸测试全绿。
5. status/doctor/reload/factory-reset 通过公共 CLI；失败不杀 PTY/server/lease。
6. 六格消费同一 `.agp` SHA；原生执行与 existence/contract-only 证据等级没有混写。
7. 一次真实 App-only CI run 证明不调用 Cargo、不重编 Base，且合同测试全绿。
8. `lint`、`check --quick` 与所有 owning smoke 在集成树上通过；文档 redaction 无命中。

任一项缺证据则保持 `[ ]`，不得用“设计已定”“可以推断”或交叉编译 existence 代替完成。

---

## 8. 决策记录

| 日期 | 决定 |
|------|------|
| 2026-08-10 | 建立 v0.1.18 独立执行草案；`plan-agenterm-app-pack.md` 继续作为完整架构与 Phase SSOT。 |
| 2026-08-10 | 本版唯一结果是 Portable App Substrate，不把 APE、多架构 loader、WASM、OTA 或真实产品迁移并入 Phase 0。 |
| 2026-08-10 | “跨平台”以同一 `.agp` 字节身份和 App-only 无 Cargo lane 为决定性证据，不宣称单一原生二进制。 |
| 2026-08-10 | QJS App ABI 采用最小 surface，不复制 Rh 全 catalog；Gate 失败不自动回退到目标相关的 Rh AOT App。 |

---

## 9. 开工检查单

1. 确认 v0.1.17 已完成或明确冻结最终未完成去向。
2. 读取本文件 §1–§5 与 `plan-agenterm-app-pack.md` 对应 Phase 0 章节。
3. 先冻结 Host ABI/manifest fixture，再写 loader 或 Engine glue。
4. 声明独占 pathspec；根 manifest、公共 schema、workflow 和 Script dispatch 串行修改。
5. cheap lint/check 先于 Cargo；App-only 变更不得借机触发 Base 全矩阵。
6. 小步提交；能力状态变化同步 owning PRD/catalog。
7. 不创建 Candidate/Promotion，除非收到明确 exact-SHA 授权。

---

*执行投影，非产品宪法。能力状态以 PRD 为准，App Pack Phase 细节以*
*[`plan-agenterm-app-pack.md`](plan-agenterm-app-pack.md) 为准。*
