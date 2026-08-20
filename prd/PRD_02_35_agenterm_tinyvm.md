# PRD 02.35 — agenterm-tinyvm

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped · `[~]` partial · `[ ]` planned

```
agenterm-tinyvm (35)
├── eval(bytes)          [x]
├── iOS runtime boundary [x]
│   ├── interpret wasm      [x]
│   ├── JIT native code     [x]
│   ├── device-side AOT     [x]
│   └── dyn native loading  [x]
├── native wasm platform [~]
│   ├── tinyvm engine       [x]
│   ├── owned host ABI      [~]
│   ├── native I/O surface  [ ]
│   └── H5/JS/WKWebView     [x]
├── game runtime         [~]
│   ├── persistent instance [x]
│   ├── start once          [x]
│   ├── per-call fuel       [x]
│   ├── memory budget       [x]
│   ├── table budget        [x]
│   └── game ABI            [ ]
├── slot-A
│   ├── control          [x]
│   ├── parametric       [x]
│   ├── locals           [x]
│   ├── memory           [x]
│   ├── i32              [x]
│   ├── i64              [x]
│   ├── f32              [x]
│   ├── f64              [x]
│   └── conv             [x]
├── host                 [x]
├── <100KiB>             [x]
├── slot-B               [ ]
└── non-goals
    ├── cu               [x]
    ├── chassis          [x]
    ├── #78              [x]
    ├── WASI             [x]
    ├── APE              [x]
    └── WAT              [x]
```

`eval(bytes)` → 值或错。程序是标准 `.wasm`。
宿主门是 import 表。未绑定即 trap。
槽 A = WASM 1.0 MVP 172 操作码。双绿。
核 strip `<100KiB`。

解释器路线是 iOS 发行边界驱动的架构选择，不是 PRD 为了缩小产品面而任意排除更快的执行方式。在目标 App Store 分发模型下，下载的 `.wasm` 只由 tinyvm 解释执行；不在设备上生成可执行原生代码，也不把下载模块编译或装载为原生动态代码。

这里排除的 AOT 特指“设备端把下载模块预编译成可执行原生代码”。开发或发布阶段把源码编译为标准 `.wasm`，以及应用本身的构建期 AOT，都不在此排除范围。`agenterm-dyn` 的本地原生动态调用面也不是 iOS tinyvm 执行路径。槽 B 暂停，直到目标分发模型与平台权限发生可验证的变化。

tinyvm 是自有 native WASM 平台的执行核，不是 H5 小游戏、JavaScript miniApp 或 WKWebView 容器。上层平台通过自有 host ABI 向 `.wasm` 提供经预算的原生渲染、输入、音频、时钟与存储面；不依赖 DOM、JavaScript 或 Web 容器语义。App Store 可接受性是该平台上层的独立发布门，不反向改写 tinyvm 的运行时架构。

JavaScriptCore 内部存在 WebAssembly 实现，但 Apple 公开的嵌入面是 `JSContext` 中的 JavaScript 执行，不是独立的原生 WASM module/instance API。JSC 可以作为后续对照基准或实验后端，但 tinyvm 是权威、可移植、可预算的 baseline；任何游戏都不得依赖 JSC 才能运行。

完整的 iOS 游戏运行底层验收树与依赖路径见 [`plan/goal-tinyvm-ios-game-runtime.md`](../plan/goal-tinyvm-ios-game-runtime.md)。
