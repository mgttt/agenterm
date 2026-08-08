# qjs 多文件 `import` 方案设计

| 字段 | 值 |
|------|-----|
| **文档** | `agenterm-qjs` 多文件项目（ES `import`/`export`）支持设计 |
| 日期 | 2026-08-08 |
| 状态 | 设计稿 rev1（未实现，仅调研+决策；见 §7 分期） |
| 关联 | `plan/plan-v0.1.16.md` §1 Rh（QJS-M3/M4 记录了本文起因的实测）、`crates/agenterm-rh/src/project_import.rs`（rh 的对应能力）、`crates/agenterm-qjs/src/check.rs`/`eval.rs`（现状代码）、PRD §「Script engine family」 |

---

## 1. 问题（已用代码实测，不是猜测）

QJS-M3 复核时发现：qjs 现在的 `entry()`-on-`globalThis` 约定和 ES
`import`/`export` **互斥**，不是「没做校验」那么轻：

```text
$ agenterm-qjs check uses_import.js     # import 一个真实存在、语法合法的文件
qjs parse error: Error: could not load module '.../lib/leaf.js'   # exit 2

$ agenterm-qjs eval uses_import.js      # 同一个文件
qjs parse error: Error: Unexpected token '{'                       # exit 2（原因完全不同）
```

根因（读 `agenterm-qjs` 源码 + rquickjs 0.12 源码确认，不是猜测）：

- `check()`（`check.rs`）用 `Module::declare` 把源码当 **ES module** 解析——
  这是 rquickjs 唯一能做到「只 parse 不执行」的路径（见 §4.3）。ES module
  链接阶段会**立刻**尝试解析并加载它 `import` 的每一个说明符；但当前
  `Context::full` 上**没有注册任何 loader**（`rquickjs` 的 `loader` 是一个
  **默认不开**的 cargo feature，见 §3），所以任何 `import` 语句——哪怕目标
  文件真实存在——都会在 `check()` 阶段直接失败。
- `eval_entry()`（`eval.rs`）把源码当**经典全局脚本**执行（`ctx.eval_with_options`，
  非 module 模式）——`import`/`export` 声明语法在经典脚本里本来就是
  SyntaxError，与「能不能找到文件」无关。

两条路径不仅**结果不同**，**失败原因也不同**，说明这不是一个小 bug，是
架构上还没有决定「qjs 的多文件项目要怎么组织」。`pack`/`qualify`
（`pack.rs`）继承的是 `eval_entry` 的经典脚本路径，同样不支持。

**好消息**：实测 `scripts/qjs/lib/fleet.js`（目前唯一随包的 qjs 脚本）不用
`import`/`export`，所以这是**潜伏缺口**，不是已发布的活 bug——设计可以
从容做，不需要抢修。

---

## 2. 核心结论（先读）

| 问题 | 结论 |
|------|------|
| qjs 要不要支持真 ES `import`/`export`？ | **要**，且用 QuickJS **原生**模块系统，不手搓文本拼接/自制 import 语法 |
| 会不会破坏现有单文件脚本（`fleet.js`、已发布测试）？ | **不会**——只对**检测到顶层 `import`/`export`** 的脚本才走 module 路径（§4.1 sniff），其余脚本的 `check`/`eval` 行为**逐字节不变** |
| `check()` 现在「总是当 module 解析」这条要不要改？ | **不改**——rquickjs 没有经典脚本的公开「只 parse 不执行」API（§4.3 已核实），module-declare 是唯一选择，这不是本设计要解的问题，是既有约束 |
| import 路径校验要不要照抄 rh 的 `project_import.rs`（文本扫描+手写越权检查）？ | **不用全抄**——用 QuickJS **真实模块链接器**做递归校验/循环检测（ES 规范原生支持循环 import，rh 的 Rhai 系统不支持所以要手写 `visiting`/`visited`，qjs 不需要）；但**越权/根目录逃逸检查要照抄**，因为 rquickjs 自带的 `FileResolver` **不提供**这个安全边界（§3 已用源码证实，不是假设） |
| 会不会需要新的 unsafe/GC 冒险代码（QJS-M2 撞过一次真崩溃）？ | **不需要**——`Runtime::set_loader`/`Module::declare`/`Module::eval`/`Module::get`/`execute_pending_job` 全部是 rquickjs 的**安全**公开 API，本设计不引入新 unsafe |
| 要不要开个新 cargo feature？ | **要**——`rquickjs` 的 `loader` 是可选 feature（`default = ["std"]` 不含它），`Cargo.toml` 需要加 `features = ["loader"]` |

---

## 3. 备选方案对比

| 方案 | 做法 | 优点 | 缺点 | 结论 |
|------|------|------|------|------|
| **A. 真 ES module（选定）** | 注册项目根目录受限的 `Resolver`+`Loader`，`import`/`export` 走 QuickJS 原生模块系统 | 循环依赖规范原生支持；每文件独立作用域（不会像文本拼接那样撞名字）；`check()` 顺带获得「递归校验整个 import 图」的能力，因为链接阶段本来就会加载所有依赖 | 需要 drain job queue（Promise）；`entry` 必须显式 `export`（对选择用 import 的脚本是新增小成本，但**只影响主动使用 import 的脚本**） | **选定**，见 §4 |
| **B. 照搬 rh 的文本拼接** | 像 `project_import.rs` 一样正则/字节扫描 `import "path"`，把依赖文件**文本拼接**成一个大脚本，走现有经典脚本执行路径 | 复用 `eval_entry` 不用碰 module/Promise | 多文件顶层 `const`/`function` 全挤一个作用域，两个文件各自声明同名局部变量会**互相覆盖**——这是 JS 有块级作用域/模块作用域之后industry 早就绕开的老坑，rh 能这样做是因为 Rhai 是自己的 import 关键字、自己的模块系统，qjs 用的是**真· JS 语法**，用真语法却手搓一个比 JS 自带模块系统更弱的拼接方案没有意义 | **不选** |
| **C. 全面切到 module 模式（连无 import 脚本也切）** | `check`/`eval`/`pack`/`qualify` 一律走 module 路径，`entry()` 一律要求 `export` | 彻底消灭 module-parse vs script-eval 的语义分裂 | **破坏性变更**：`fleet.js`、已发布的 CLI 约定、19+41 个既有测试的「顶层 `function entry()`」写法全部作废；且 §4.3 已证实经典脚本没有等价的「只 parse 不执行」API，`check()` 切换后若不用 module 就无法保留「不执行副作用」这条已测试锁住的保证 | **不选**（成本/收益倒挂，且 §4.3 的技术约束让它比看起来更难） |

---

## 4. 选定方案（A）细节

### 4.1 顶层 `import`/`export` 探测（sniff）

不需要精确的语法分析器——只是一个**路由**决策，真正的语法合法性交给
QuickJS 自己的 parser 把关（sniff 误判的后果只是走错分支，得到一个
QuickJS 原生的语法错误，不会静默错误执行）。做法与 rh 的
`project_import.rs::literal_imports` 同源：跳过字符串/注释，扫描是否有
一个**语句起始位置**的 `import` 或 `export` token。命中→走 module 路径；
未命中→现有经典脚本路径**逐字节不变**。

（次要考虑过、放弃的替代方案：先按经典脚本 eval，失败后重试 module——
两次 parse 更贵，且报错信息会变得模糊「到底是真语法错还是该走 module」；
sniff 更便宜也更诚实。）

### 4.2 项目根目录受限的 `Resolver`/`Loader`

**已用 rquickjs 源码验证、不是假设**：`rquickjs::loader::FileResolver`
的 `resolve()`（`file_resolver.rs`）用 `RelativePath::join_normalized`
把 `..` 段**规范化**到 base 目录，但**不裁剪到任何根目录**——`import
"../../../etc/passwd.js"` 这类路径，`FileResolver` 会老老实实算出规范化
后的（可能已经跑出项目树的）路径，交给 `ScriptLoader` 直接
`std::fs::read`。这不满足这个引擎已经在别处承诺的不变量（`check_many`
已有 `check_many_rejects_absolute_paths_outside_project_root` 这类测试；
rh 的 `project_import.rs` 有 `rejects_import_root_escape`）。

所以：**不能裸用 `FileResolver`**。要写一个薄的自定义 `Resolver`
（实现 `rquickjs::loader::Resolver` trait），语义照抄
`project_import.rs::checked_module_file` 的检查顺序：

1. 拒绝空/超长（>4096 字节）说明符；
2. 拒绝绝对路径说明符；
3. 拒绝含 `..`/`RootDir`/`Prefix` 分量的说明符；
4. 相对 `base`（当前模块的路径）解析出候选路径；
5. `fs::canonicalize` 候选路径，`starts_with(project_root)` 校验没有
   通过符号链接等方式逃逸；
6. 通过后，`Loader::load` 侧复用 `rquickjs::loader::ScriptLoader`
   （读文件→`Module::declare`）即可，不用重新发明加载逻辑，只是**resolve
   这一步不能相信第三方默认实现**。

`project_root` **必填**：脚本一旦命中 sniff（要用 `import`/`export`），
调用方必须提供 project root，否则直接拒绝并给出诚实的错误信息（例如
`qjs_module_no_project_root: source uses import/export but no
project_root was provided`），而不是悄悄退化成不受限解析。

### 4.3 「`check()` 保持不变」这条结论的技术依据

已读 rquickjs 0.12 源码核实：`Ctx::eval_raw`（唯一能做 `JS_EVAL_FLAG_COMPILE_ONLY`
经典脚本编译的入口）是 `pub(crate)`，不对外暴露；公开 API 里唯一「只编译
不执行」的入口是 `Module::declare`（模块路径）。也就是说，`check()`
「不执行顶层副作用，只校验语法」这条**已经测试锁住**的保证
（`does_not_execute_top_level_side_effects`），目前**只能**通过
module-declare 拿到——如果哪天要让 `check()` 对非-module 脚本也做到
「parse-only」，需要的是 rquickjs 上游开一个新的公开 API，不是本设计能
绕过的事。所以本方案范围内，`check()` 对**没有** `import`/`export`
的脚本继续用 `Module::declare`（现状不变，19 个既有 `check`/`check_many`
测试不用碰）；对**有** `import`/`export` 的脚本，`Module::declare`
本来就会触发链接阶段递归加载它引用的每个文件——**只要注册了 §4.2 的
受限 resolver/loader，`check()` 就顺带获得了「递归校验整个 import 图」的
能力**，不需要另外写一套类似 `project_import.rs` 的图遍历代码。这是选
方案 A 而不是 B 的另一层收益，之前没预料到，是读源码过程中发现的。

### 4.4 执行路径（`eval`/`run`/`pack`/`qualify`）

对 sniff 命中的脚本：

1. 按 §4.2 注册 resolver/loader 的 `Runtime`；
2. `Module::declare(ctx, label, source)` → `.eval()`，拿到
   `(Module<Evaluated>, Promise)`；
3. drain job queue 直到 promise 落定：
   `while runtime.is_job_pending() { runtime.execute_pending_job()... }`
   （`execute_pending_job`/`is_job_pending` 都是 `Runtime` 的公开安全
   方法，`runtime/base.rs` 已核实存在）；
4. promise 落定后若是 rejected（模块顶层求值抛异常），按现有
   `QjsError` 分类抛出（等价于经典脚本路径里「entry() 之前的顶层代码
   炸了」）；
5. `module.get::<Function, _>("entry")`（`Module<Evaluated>::get`，已核实
   存在）——拿不到（脚本忘记 `export`）时按现有「无 top-level entry()」
   的 fail-closed 约定报错，只是错误文案从「挂在 globalThis 上」换成
   「没有 export」；
6. 调用 `entry()`，走**和经典脚本路径完全相同**的
   call→catch→json_stringify 尾段（`eval.rs` 现有代码，不用改）。

`__host`/`print` 不用动：它们是 `ctx.globals().set(...)` 挂的全局绑定，
module 作用域的代码一样能读全局（只是模块顶层的裸 `var`/`function`
声明不会**写**到 globalThis，读/调用现有全局属性不受影响）。

### 4.5 `pack`/`qualify` 呢

`pack.rs` 现在的「重新解析 source 执行」策略（不走字节码加载，见
`pack.rs` 模块注释）本身不变——本设计只是让「重新解析 source 执行」这
一步在 sniff 命中时切换成 §4.4 的 module 路径，而不是经典脚本路径。
`compile.rs` 的「真字节码指纹」（`Module::declare(...).write(...)`）本来
就已经是 module-mode 编译，这条**已经**天然兼容本设计——sniff 命中的
脚本编译出的字节码指纹现在会是「已解析、已链接」的 module 字节码而不是
「未解析 import 就失败」的产物，指纹语义不变，只是终于能对含 import 的
脚本算出一个有意义的指纹了。

---

## 5. 与 rh `project_import.rs` 的对比

| 维度 | rh | qjs（本设计） |
|------|----|----|
| import 语法 | Rhai 自定义 `import "path" as name;`（不是任何 JS 语法） | 真 ES `import`/`export` |
| 解析方式 | 手写字节扫描找 `import` 字面量（`literal_imports`） | 同款字节扫描，但只用来做**路由**（sniff），不用来做**解析**——真解析交给 QuickJS |
| 循环依赖 | 手写 `visiting`/`visited` 检测，报 `script_module_cycle` | ES 规范原生支持循环 import（live binding），**不需要**手写检测 |
| 越权/根目录逃逸 | 手写 `checked_module_file`（拒绝 `..`/绝对路径，canonicalize + `starts_with`） | **同款检查**，但套在 rquickjs 的 `Resolver` trait 里（`FileResolver` 不提供，已用源码证实，§4.2） |
| 每个依赖文件的语法校验 | 手写：对每个 import 到的文件调用 `parse_rh_ast` | **免费**：QuickJS 模块链接器本来就会 parse 它加载的每个文件 |
| 作用域隔离 | Rhai 有自己的模块作用域规则 | ES module 原生作用域隔离（也是不选方案 B 文本拼接的原因） |

**一句话**：qjs 不是「照抄 rh 的实现」，是「借用 rh 已经验证过的**安全策略**
（越权检查、必须显式提供 project_root），套进 QuickJS 自己更强的原生模块
系统里」——省掉的是 rh 因为 Rhai 没有原生模块系统而不得不手写的部分
（循环检测、递归 parse），保留的是两边都需要的部分（安全边界）。

---

## 6. 风险 / 非目标

- **非目标**：动态 `import()` 表达式（运行时条件加载）——本设计只覆盖
  静态顶层 `import`/`export` 声明；动态 `import()` 在经典脚本里语法本来
  就合法（不需要 module 模式），暂不特别处理，也不是这次要解决的问题。
- **非目标**：把 `fleet.js` 迁移成 `export const fleet = {...}` 风格——
  它现在不用 import/export，本设计不强迫任何现有脚本改写。
  是否要在未来把 L2 facade 库本身做成可 `import` 的模块是另一个独立
  决策，不在本文范围。
- **风险**：job-queue draining 是这个 crate **第一次**接触 QuickJS 的
  Promise/microtask 机制——虽然全程走的是安全 API，但 QJS-M2 的教训是
  「编译过 ≠ 没有内存/生命周期坑」，drain 循环、`Ctx` 生命周期在
  跨 job 场景下的行为需要专门的最小复现测试再合入，不能只靠「文档说
  这样用」就当作已验证。
- **风险**：`loader` feature 会把 `relative-path` crate 拉进依赖树
  （`rquickjs-core` 的 `loader = ["relative-path"]`）——影响面小，但要
  在实现阶段过一遍 `cargo check --workspace`，不能假设「加个 feature
  flag 肯定没事」。

---

## 7. 实现分期（本文档只覆盆设计，以下留给后续实现叶）

| 叶 | 内容 |
|----|------|
| **QJS-M5a** | `Cargo.toml` 加 `features = ["loader"]`；新增受限 `Resolver`（§4.2），配套单测（合法相对 import、`..` 逃逸拒绝、绝对路径拒绝、符号链接逃逸拒绝、循环 import 两文件互相 import 应该成功而不是报错——反向验证 §5 的「循环依赖免费支持」结论） |
| **QJS-M5b** | sniff 函数（§4.1）+ module 执行路径（§4.4：declare→eval→drain→get(entry)→call），先接到 `eval_entry`/`eval.rs`，配套单测（单文件 import 一个依赖跑通、依赖循环跑通、忘记 export entry 报错、依赖里有语法错误时报错定位到正确文件） |
| **QJS-M5c** | `check()` 接上 §4.2 的 resolver（无 import 脚本行为不变，有 import 脚本递归校验），`pack.rs`/`qualify.rs` 接上 §4.5 | 
| **QJS-M5d** | 端到端 CLI smoke（真实多文件目录，`check`/`eval`/`run`/`pack`/`qualify` 全过）+ 更新 `plan-v0.1.16.md`/`lib.rs` 模块注释里「多文件 import 不支持」的记录 |

不在本次设计范围内、留给需要时再议：是否要给 `scripts/qjs/lib/fleet.js`
本身也提供一个 `export` 版本供多文件脚本 `import`。
