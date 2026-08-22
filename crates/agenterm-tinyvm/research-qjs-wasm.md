# qjs-wasm 路径（只读 · 吃设计 · 不是写刀）

对照：`crates/agenterm-tinyvm/src/wasm.rs` 脸；`/workspace/ceo/plans/tinyvm-plugin-host.md`；
`/workspace/ceo/reviews/tinyvm-plugin-face-review-prep.md`。Bellard / quickjs-ng / quickjs-wasi
只吃设计，不搬代码、不 clone。仓停在 `origin/main`，本文件是唯一新建。

能否：能但只长宿主调度与插件；`eval(bytes)` 仍只吃 wasm，完整 JS 不运行时 AOT。
qjs-wasm 独立：独立于 tinyvm 且不并进现成 `agenterm-qjs`；同 workspace 新 crate/插件件即可，不必另起 git 第三仓。

```
路径
├── 不是当前写刀 [x]
│   ├── 不改 crates 源码 / 测试 / PRD
│   ├── 不开 eval_plugin、不开 slot-B、不碰 dyn / #78
│   └── 本页只答能不能长、怎么挂、独立与否
│
├── 现成三件（禁止当绿地）
│   ├── tinyvm 脸 [x]
│   │   ├── `eval` / `eval_with` / `Module::from_bytes` / `from_bytes_with`
│   │   ├── 装载期校验 + `Limits`；禁 JIT；必须能上 iOS / wasm32
│   │   └── 一个 `.wasm` = 一个插件（wasm bin pack）；插件只经宿主门，互不见
│   ├── agenterm-qjs [x]  ·  crates/agenterm-qjs
│   │   ├── 本机 rquickjs，对齐 rh / lua 的 check/eval/pack
│   │   ├── Cargo/lib 钉死：Explicitly NOT AOT/native codegen
│   │   ├── pack 指纹是 qjsc 字节码，执行仍重解析源码（信任脚本模型）
│   │   └── 不是 tinyvm 上的 wasm 插件
│   └── 插件宿主口径 [x]  ·  核只装/卸/依赖/列表；能力在插件；无 JS Context
│
├── 问 1 · 脸能不能长在现成 tinyvm 上
│   ├── 能：Python `eval(data, globals, locals)` 的类比落在宿主层，不拆核脸
│   │   ├── `data` 默认 wasm 字节 → 现成 `eval(bytes)` / `eval_with(..., Limits)`
│   │   ├── `globals`/`locals` → 宿主门（import 表 / 将来 HostGate），不是第二套 FFI
│   │   └── 语言调度（js / DSL）在 tinyvm 之上：核仍只认 `\0asm`
│   ├── 不拆四条
│   │   ├── 不给 `eval` 加 JS 输入；WAT / 源码不是入口
│   │   ├── 不 JIT、不 slot-B；iOS / wasm32 继续解释执行
│   │   ├── 一个 wasm 一个插件：客人仍是一份 `.wasm` 一份槽
│   │   └── 插件互不见：JS 引擎也只 import 宿主门，不得 import 他插件
│   └── 预审提醒（不本页开刀）：插件脸若另立 `eval_plugin`，内部仍走 `from_bytes_with`；
│       能力闸 ≠ `Limits`。语言调度更在那一层之上。
│
├── 问 2 · 完整 JS：运行时 AOT→wasm 不现实
│   ├── 不现实（理由，不是空喊）
│   │   ├── Bellard 设计：QuickJS 是「解析 → 自有字节码 → 解释」；qjsc「编成可执行」
│   │   │   是把字节码嵌进仍链解释器的 C，不是把 JS 语义降成 wasm 操作码
│   │   ├── JS 动态类型：`x+y` 是数加 / 拼串 / valueOf / Proxy……AOT 到 MVP wasm
│   │   │   必须自带对象模型、GC、eval、正则；那就是再做一个引擎，不是转换器
│   │   ├── 公开线（Javy / spin-js / quickjs-wasi）全是「解释器编进 wasm」，
│   │   │   不是「源码 AOT 成无运行时的 .wasm」；quickjs-ng 的 WASI/Emscripten 同此
│   │   ├── WasmGC / 异常 / 向模块热加函数：tinyvm 核是 WASM 1.0 MVP 解释器，
│   │   │   且禁 JIT；运行期不能往已装模块塞新机器码，也不吃 WasmGC 提案
│   │   └── 真 AOT JS→wasm（SpiderMonkey baseline / js2wasm 一类）依赖 IC 宿主或
│   │       GC 提案，且完整 ES 未收口；不能当 tinyvm 客人编译器
│   ├── 现实挂法：引擎插件
│   │   ├── 构建期：QuickJS C 解释器 → 一份 `qjs.wasm`（reactor 形，吃设计）
│   │   ├── 进宿主：该 `.wasm` 走同一套 `eval(bytes)` + validate + `Limits`
│   │   ├── 运行期：JS 源码是线性内存里的数据，经宿主门喂给引擎；不是第二次 eval(JS)
│   │   ├── 一插件一客人：要「一份不可信 JS 应用 = 一份插件」，在 pack 期把源码
│   │   │   嵌进引擎 wasm（Wizer/qjsc 嵌字节码那类），产出仍是一份 `.wasm`
│   │   └── 双解释：tinyvm 解释 wasm，wasm 里再解释 JS。合法（两边都无 JIT），
│   │       但默认 `max_steps` / `WASM_MAX_DECODE_ITEMS` / 核 `<100KiB` 都可能不够——
│   │       那是 Limits 与插件体积，不是改脸。公开 WASI 构建约兆级，不得进 staticcore
│   └── ABI：插件只绑宿主门，不把 wasi-p1 POSIX 当面。tinyvm 的 `wasi-p1` 是可选适配，
│       不是插件模型。引擎 wasm 的 import 必须是门名单（clock/print/eval-in 一类），
│       不得把 fd_* 做成第二扇 OS 面。
│
├── 问 3 · 小 DSL：转换器插件 vs 引擎插件
│   ├── 转换器插件
│   │   ├── 吃：小 DSL 源码（静态、无 `eval`、类型/操作有穷）
│   │   ├── 吐：标准 `.wasm` 字节
│   │   ├── 谁进 tinyvm eval：吐出的那份 wasm（第二次过脸：validate + Limits）
│   │   ├── 转换器自己也是一份 `.wasm` 插件，先过脸；两槽，不直连，宿主转发
│   │   └── 适用：表达式、配置、WAT 子集、有穷状态机。完整 JS 不准冒充转换器
│   ├── 引擎插件
│   │   ├── 吃：源码 + 入参（经门写入自己的线性内存）
│   │   ├── 吐：值 / 错（经门回宿主）；不吐「可再 eval 的 wasm」为产品路径
│   │   ├── 谁进 tinyvm eval：引擎这份 `.wasm`（一次）
│   │   └── 适用：完整 JS、需要 GC/对象/闭包的语言
│   └── 切：能不能在不带该语言运行时的前提下，把源码降成 MVP wasm 操作序列。
│       能 → 转换器；不能 → 引擎。JS 落后者。
│
├── 问 4 · qjs-wasm 该不该独立
│   ├── 不进 tinyvm crate
│   │   ├── 核脸是解释器 + 校验 + Limits + `<100KiB` staticcore
│   │   ├── 引擎是客人：C 工具链、兆级 wasm、许可证与构建图都不该进核
│   │   └── 塞进去会把 WASI/libc/JS 语义长进「第三产品」核，违反插件宿主「能力全在插件」
│   ├── 不并进现成 agenterm-qjs
│   │   ├── 那条是本机信任脚本（fleet_call / worker / pack 目录），rquickjs 链本机
│   │   ├── 本条是不可信字节客人，只经 tinyvm 脸与宿主门
│   │   └── 并进会把信任模型、ABI、禁 AOT 声明三条拧成一条，且仍要外挂 wasm 产物
│   └── 第三件 = 插件件 / 新 crate，不是第四脚本引擎（不进 rh/lua/qjs 矩阵）
│       ├── 边界：tinyvm 只提供 eval+校验+Limits+门；qjs-wasm 提供引擎 `.wasm` 与 pack
│       └── 仓位：同 workspace 新 crate（或 guests 外的插件包）即可；现在不必另起 git
│
└── 问 5 · 一页收口
    ├── 能长在现成 tinyvm 上，条件是不拆脸、不 JIT、JS 当引擎插件
    ├── qjs-wasm 独立于 tinyvm 与本机 qjs crate；不是当前写刀
    └── 下一刀若有：仍是插件宿主脸 / 诊断 / 门收口，不是本页开编 qjs
```

公开设计锚（不搬仓、不抄源）：Bellard QuickJS = 字节码解释器 + qjsc 嵌字节码；
quickjs-ng 平台表含 iOS / WASI / Emscripten（解释器交叉编译，非 JS→wasm AOT）；
quickjs-wasi 一类 = 解释器编成 reactor wasm + 少量 import（WASI + host_call）。
