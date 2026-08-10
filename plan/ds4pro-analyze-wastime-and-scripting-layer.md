# wasmtime 与脚本层统一分析（rev2）

| 字段 | 值 |
|------|-----|
| **文档** | wasmtime 整合对脚本层统一的影响分析：能否用 WASM 替代 QJS+rh 双引擎 |
| **日期** | 2026-08-10（rev2：推翻"做不了"的草率判断，JIT/AOT/FFI 在手里没有做不了的） |
| **状态** | 分析稿 |
| **前置** | `plan/plan-v0.1.18.md` §1.5/§1.9、`crates/agenterm-wasmcore/README.md`、`plan/ARCHITECTURE.md` |

---

## 1. 一句话

**技术上有 JIT/AOT/FFI 在手里，WASM 统一脚本层不存在"做不了"——但这不意味着"应该做"。真正的问题是：统一的代价是什么，收益能不能覆盖代价。**

---

## 2. 纠正 rev1 的错误

rev1 犯了三个草率判断：

**错 1：「WASM 编译破坏 hot reload 秒级闭环」**

真实情况：wasmtime 的 Cranelift 对小模块编译极快，且有 `wasmtime::Config::cache_config_load` 持久化缓存——首次编译几百 ms，缓存命中后加载接近零成本。加上 `wasmtime compile --cache` AOT 预编译 → `.cwasm` 即载即用。秒级闭环完全做得到，只是链路从 `save .js → reload` 变成 `save .rs/.ts → compile → reload`。

**错 2：「.wasm 二进制不可审计」**

真实情况：源码可以一起分发。厂商发布 `.agp` 时附带源码目录 + 可重现构建指令（固定工具链版本 → SHA 校验），用户可以 inspect 源码后自行编译验证二进制是否匹配。这不是做不了，是分发模型的设计问题。

**错 3：「WASM 替代 rh 只有编译成本没有收益」**

真实情况：用 `wasmtime compile` 预编译 rh 脚本 → `.cwasm` 即载即用，和今天的 rh AOT 在体验上没有本质区别。成本是一次性编译，收益是统一引擎、去掉一个 runtime 的维护负担。

---

## 3. wasmcore 现状（树）

```text
wasmcore 机制验证已完成（独立 crate，未接入产品路径）
├── 3.1 已实现
│   ├── wasmtime + Cranelift JIT → 加载 wasm32-wasip1 模块
│   ├── 暴露 fleet_call(op_id, params_json) → Result<result_json, error> 桥
│   ├── 与 QJS/rh/lua 共享同一 ScriptFleetBridgeFn trait
│   ├── 端到端 roundtrip 测试通过
│   └── Windows 栈溢出修复（16 MiB worker 线程栈）
│
├── 3.2 未实现（本阶段明确排除）
│   ├── 未接入 execute_inner / 产品脚本路径
│   ├── 不在根 workspace（独立 Cargo.toml + 独立 Cargo.lock）
│   └── 不涉及 wasm64 / WASI p2 / component model
│
└── 3.3 与四个 agenterm-*core crate 的关系
    ├── nativecore: 直接 Win32 调用（Windows-only, 无 JIT）
    ├── guestcore: x86_64 指令解释器（同 ISA，无 JIT）
    ├── dynacore: 自研字节码 VM（ISA 中立，无 JIT）
    └── wasmcore: wasmtime JIT（ISA 中立，复用成熟运行时）
```

---

## 4. 如果统一到 WASM：两种可行架构（树）

```text
JIT/AOT/FFI 在手里，不存在技术做不成的方案
├── 4.1 方案 A：语言运行时编译为 WASM（wasmtime 为统一宿主）
│   │
│   │   wasmtime (单宿主引擎)
│   │   ├── qjs.wasm (QuickJS 编译为 wasm32-wasip1) → 跑 .js app pack
│   │   ├── rhai.wasm (Rhai 编译为 wasm32-wasip1) → 跑 .rh build 脚本
│   │   └── custom.wasm → 用户扩展
│   │
│   ├── 优势
│   │   ├── 单一宿主引擎维护
│   │   ├── 每个语言运行时独立更新（不影响宿主）
│   │   └── 统一 WASM 沙箱模型
│   │
│   ├── 代价
│   │   ├── qjs.wasm 比 native QJS 慢（WASM 内解释 JS，双重开销）
│   │   ├── rhai.wasm 比 native Rhai AOT 慢
│   │   ├── app pack 变更 = 更新 qjs.wasm 容器（容器也是二进制）
│   │   └── 源码可审计性降级到"容器内的脚本 + 容器本身可审计"
│   │
│   └── 判断：收益 = 宿主统一，代价 = 所有脚本慢一圈。不值得。
│
└── 4.2 方案 B：所有脚本直接编译为 WASM（wasmtime 为唯一引擎）
    │
    │   wasmtime (唯一引擎)
    │   ├── .rh → Rhai 编译器 → .wasm → wasmtime 执行
    │   ├── .ts/.rs → Rust/AS 编译器 → .wasm → wasmtime 执行
    │   └── .cwasm (AOT 预编译) → 即载即用
    │
    ├── 热重载
    │   ├── 开发模式：watch 源码 → compile → wasmtime 加载
    │   ├── 首次编译：Cranelift 几百 ms（小模块）
    │   ├── 缓存命中：wasmtime cache + AOT .cwasm → 接近零成本
    │   └── 结论：秒级闭环**做得到**，但不等于 QJS 的零步 reload
    │
    ├── 源码可审计
    │   ├── .agp 封包 = 源码 + 预编译 .wasm + 可重现构建指令
    │   ├── 用户 inspect 源码 → 自行编译 → 比对 .wasm SHA
    │   └── 结论：可审计**做得到**，但比"直接看 .js"多一步
    │
    ├── CI 脚本零成本
    │   ├── `wasmtime compile build.rh.wasm → build.rh.cwasm`
    │   ├── CI 加载 .cwasm 零延迟
    │   └── 结论：和今天的 rh AOT 体验无本质差异
    │
    ├── 优势
    │   ├── 单一引擎维护（去掉 QJS + rh 两个运行时）
    │   ├── 语言无关（用户用 Rust/Go/C/Zig/AS 写扩展）
    │   ├── 统一安全模型（WASM import 白名单）
    │   ├── WASM 生态直接复用
    │   └── wasmtime 由 Bytecode Alliance 维护（10+ 年成熟度）
    │
    └── 代价
        ├── 开发闭环多了编译步骤（不再是 save .js → reload）
        ├── 源码分发需要附带可重现构建指令（不再是"源码即可执行"）
        ├── wasmtime 依赖重量（Cranelift + wasmtime-wasi）进入 Base 二进制
        ├── 现有 QJS/rh 代码需要重写（app pack → TS/AS，build → Rust→WASM）
        └── 调试体验：.wasm 栈回溯 vs .js/.rh 源码级错误（可解决，但有差距）
```

---

## 5. 代价量化：方案 B 的具体账单

```text
统一到 WASM（方案 B）的一次性成本和持续成本
├── 5.1 一次性成本
│   ├── app pack QJS 代码 → TypeScript/AssemblyScript 重写
│   │   ├── 现有资产：cc/ shell/ settings/ llm/ theme/ → 全部重写
│   │   └── 估计：保守 2–4 周专人工时
│   │
│   ├── Build/CI rh 脚本 → Rust→WASM 重写
│   │   ├── scripts/rh/build.rh, check.rh, release.rh, lint.rh, *.rh
│   │   ├── rh 的 fleet.* 有 44 个操作 → WASM guest 需要等价的调用层
│   │   └── 估计：1–2 周专人工时
│   │
│   ├── wasmtime + wasmtime-wasi 进入根 workspace
│   │   ├── 当前独立 Cargo.lock（避免污染根 workspace）
│   │   ├── 接入后根 Cargo.lock 膨胀（Cranelift 传递依赖树）
│   │   └── 首次 clean build 时间增加（估计 +30–60s）
│   │
│   └── 分发模型改造
│       ├── .agp 从"打包源码"变为"源码 + 预编译 .wasm + build 指令"
│       ├── CI 新增 wasm32-wasip1 编译矩阵
│       └── OTA 更新流程从"推送 .js"变为"推送 .wasm + 验证签名"
│
└── 5.2 持续成本
    ├── 开发闭环：每次改脚本 → 编译 .wasm（比 save .js→reload 慢一个编译步骤）
    ├── 调试：.wasm 栈回溯不如 .js 源码错误直接
    │   └── 可缓解：wasmtime + DWARF 调试信息 → source map 回源码行号
    ├── Base 二进制体积：Cranelift JIT 接入增加（估计 +1–2 MiB）
    │   └── 当前 agenterm.exe 预算 4 MiB → 需要评估是否超标
    └── 学习成本：用户写扩展需要 wasm32-wasip1 工具链
```

---

## 6. 统一 vs 不统一：决策树

```text
            是否统一到 WASM？
           /               \
        统一              不统一（现状：QJS + rh 双引擎）
       /     \                  |
    收益      代价          QJS: app pack 产品面
    /           \           rh: Build/CI
单一引擎      重写代码      wasmcore: 用户扩展（第三引擎）
语言无关      编译步骤
WASM生态      依赖体积
统一沙箱      调试落差
```

### 6.1 什么时候统一是对的？

```text
触发条件（满足任一则统一收益 > 代价）
├── 用户写扩展的语言需求爆发（JS 不够，Rust/Go/C 需求成规模）
├── QJS 性能成为 app pack 瓶颈（实测数据，不是猜测）
├── rh 维护成本超过了"重写所有 rh 脚本"的成本
└── 跨语言安全隔离成为硬需求（WASM sandbox 比引擎级沙箱更底层、更可验证）
```

### 6.2 今天不统一的理由（不是"做不了"）

```text
今天不统一的理由 = 代价不划算，不是技术不可能
├── 1. QJS + rh 已经工作 → 先证明统一的价值，再付重写的成本
├── 2. app pack 还在 Phase 0 占位 → 等 QJS 上线、收集用户反馈后再判断
├── 3. wasmcore 还未接入产品路径 → 先跑通三引擎共存，再谈砍引擎
├── 4. 重写的专人工时 > 维护双引擎的边际成本（当前）
└── 5. 编译步骤对"改空态文案"这种高频小改是纯摩擦
```

---

## 7. 当前建议：三引擎共存，用数据说话

```text
当前行动（不砍任何人，各自先跑起来）
├── 7.1 QJS（app pack）
│   ├── Phase 0 占位 → Phase 1 单回调 → Phase 2 CC chrome
│   └── 目标：上线后收集热重载/可审计/OTA 的实际用户反馈
│
├── 7.2 rh（Build/CI）
│   ├── 保持 scripts/rh/ 资产不动
│   └── 目标：维护成本自然摊销（不改就不花工时）
│
├── 7.3 wasmcore（用户扩展 + 性能通道）
│   ├── 接入产品路径（execute_inner / ScriptEngineBackend）
│   ├── 纳入根 workspace
│   ├── 发布 SDK（wasm32-wasip1 编译指令 + fleet_call ABI 文档）
│   └── 目标：让用户能用 Rust 写第一个 WASM 扩展，收集真实需求
│
└── 7.4 决策点：v0.2.x 回顾
    ├── QJS app pack 的实际热重载延迟（> 200ms？= 瓶颈？）
    ├── WASM 扩展的实际采用量（用户有没有真的写 WASM？）
    ├── rh 维护的边际成本（有新需求吗？还是纯惯性维护？）
    └── 数据说话：谁跑得好留谁，跑不好的考虑砍
```

---

## 8. 长期可能方向

```text
如果数据指向统一，优先级从高到低：
├── 方向 A：QJS + rh + wasmcore 各自优化 → 维持现状，不统一
│   └── 适用：三引擎各自满足需求，边际维护成本低
│
├── 方向 B：砍 rh，Build/CI 转 WASM（rh 脚本重写为 Rust→WASM）
│   └── 触发：rh 维护成本高，且 CI 脚本变更频次低
│
├── 方向 C：砍 QJS，app pack 转 WASM（JS 脚本重写为 TS/AS→WASM）
│   └── 触发：QJS 性能瓶颈 + 用户愿意接受编译步骤
│
└── 方向 D：全砍，wasmtime 为唯一引擎
    └── 触发：WASM 扩展需求爆发 + 双引擎维护成本不可持续
```

---

## 9. 交叉引用

- **引擎策略 SSOT**: `plan/plan-v0.1.18.md` §1.5/§1.9
- **wasmcore 机制验证**: `crates/agenterm-wasmcore/README.md`
- **wasmcore ABI 规格**: `crates/agenterm-wasmcore/README.md` "fleet_call calling convention"
- **结构总图**: `plan/ARCHITECTURE.md`

---

## 10. 修订记录

| 日期 | 变更 |
|------|------|
| 2026-08-10 | 初稿（rev1）：草率判"做不了"——❌ |
| 2026-08-10 | rev2：推翻 rev1，JIT/AOT/FFI 在手里没有做不了的。重写为"代价 vs 收益"决策分析 |
