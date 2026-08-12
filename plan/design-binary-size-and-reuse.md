# agenterm.exe 体积归因与抽象/复用路线

> 2026-08-09。起因:dist 里 agenterm.exe 15MB、target/debug 35MB,怀疑有大量抽象与复用空间。
> 本文用实测回答"15MB 是什么构成的",并给出按收益排序的复用路线。

## 0. 结论先行

1. **dist 的 15MB 不是发布体积**:`dist/agenterm.json` 写明 `"profile": "dev"`——本地
   `stage-build dev` 暂存的是 debug 产物。体积预算门(`artifact-verification.rh`)只在
   release 通道生效,所以这个数字从未被合同约束过。
2. **发布形态的主要质量不在"重复抽象",在"引擎搭载策略"**:自有代码只占 .text 的 1/4;
   四个静态链入的脚本引擎(rhai/LuaJIT/QuickJS/sqlparser)合计约占一半。
3. **最大单项是尚无执行能力的 sql 脚手架**(sqlparser,.text 的 22%)。把它移出产品 PE
   是收益最大、风险最小的一刀。
4. **合同已经被击穿,只是引信未点**:当前 HEAD 的真 release 构建(opt-z + thin LTO +
   cu=1 + strip)实测 7,516,672 字节 = **7.17MiB**,超出 4MiB 预算 79%。预算门只在
   release 通道执行,本地 dev 链永远不会触发——第一次走 release 通道就会
   `artifact_release_budget` 失败。引擎门控因此不是优化,是履约。
5. 现有运行时抽象是健康的:`ScriptEngineBackend` trait 四后端各就各位,平台层
   (agenterm-platform)按 windows-sys 直调,GUI 栈只有 ~3%。缺的是**编译期门控**
   (root crate 没有任何 `[features]`),不是缺 trait。

## 1. 测量方法与数据

`cargo bloat --profile release-fast --crates --bin agenterm`(strip=none 以保留符号;
独立 CARGO_TARGET_DIR 冷构建,2026-08-09)。release-fast 无 LTO、cu=16、增量开启,
绝对值偏大,**占比**才是结论载体。.text 共 7.0MiB:

| 贡献者 | .text | 占比 | 备注 |
|---|---:|---:|---|
| agenterm(src/ 自有代码) | 1.7MiB | 25% | |
| sqlparser | 1.5MiB | 22% | `agenterm sql` 脚手架;eval 尚 fail-closed |
| rhai + agenterm_rh | ~1.2MiB | 17% | 任务系统承重,不可拆 |
| LuaJIT + QuickJS(C 静态库,含 762KiB 无名行)+ mlua/rquickjs 胶水 | ~1.1MiB | 15% | |
| std | 526KiB | 7% | |
| GUI 栈(winit/softbuffer/ttf_parser/ab_glyph/png) | ~220KiB | 3% | 出乎意料地薄 |
| 其余(serde_json 34KiB、vt100 12KiB 等长尾) | ~0.7MiB | 10% | 无单项 >50KiB |

参照物:
- **真 release 实测**(独立 target 冷构建,同日):agenterm.exe = 7,516,672 字节 =
  7.17MiB。这是当前 HEAD 的可分发形态。
- 体积预算合同:PRD_02_19 G3 已升格——GUI 4MiB、sidecar 2MiB,`scripts/artifacts.json`
  的 `release_budget_bytes` + `artifact-verification.rh`(仅 release 通道)强制。
  7.17MiB 对 4MiB:即便 P1+P2 全部落地(约 −2.5MiB .text 量级),也只是逼近而未必
  跌回预算内——预算数字本身是否要随"单 PE 多引擎"的产品决定重议,需要一次明确裁决。
- 引擎并入前的 release agenterm.exe 约 1.0MiB(2026-08-04 存档产物),可视为
  "产品本体 + GUI 栈"在 opt-z + thin LTO 下的历史基线。
- debug 30MB+ 属正常(debug=1 行号表 + 未优化),与复用无关,不必优化。

## 2. 按收益排序的路线

### P1 把 sql 脚手架移出产品 PE(−22%,低风险)
`agenterm sql` 目前 check 真实、execute fail-closed,产品价值为零,但 sqlparser 是
最大单项。做法二选一:
- root `[features] engine-sql`(默认关),`src/bin/agenterm.rs` 的 `"sql"` 分支与
  `SqlEngineBackend` 挂 `#[cfg(feature = "engine-sql")]`;
- 或干脆只保留 `agenterm-sql` sidecar bin,产品 PE 不再链接。
执行能力落地那天再默认打开,符合"脚手架不进产品"的一致口径。
可行性注:`SqlEngineBackend::enabled()` 的**运行时门已存在且默认关**(须
`AGENTERM_SCRIPT_BACKEND=sql` 显式启用)——P1 只是把既有语义上移到编译期,默认行为
不变。改动面:root Cargo.toml(optional dep + feature + bin required-features)、
src/bin/agenterm.rs 的 sql 分支、script_engine.rs / script_backend.rs / script_worker.rs
的 Sql 变体,及 3 个 parity 测试。这些文件当前有并行 lane 的未提交改动,实现宜在其
落地后进行,避免冲突。

### P2 引擎门控成为一等机制(策略对齐 roadmap)
路线图(plan-v0.1.16.md §1)本来就是分平台的:rh(Linux 主力)、lua(Windows)、
qjs(等 lua 原型验证)。但 root crate 无 feature 门,四引擎无条件全量链入所有平台。
建议:`engine-lua` / `engine-qjs` / `engine-sql` 三个 feature(rh 承重不设门),
`ScriptEngineBackend` 注册表按 feature 组装。这使"哪个平台带哪个引擎"从文档约定
变成构建事实,也让 4MiB 预算在引擎继续增多时仍可能守住。

### P3 依赖卫生:引擎 crate 一律经 script-common 取哈希/扫描
实测重复:sha2 0.10(lua/qjs 声明)与 0.11(root/rh)双链并存,连带 digest /
block-buffer / crypto-common / cpufeatures 各双份;另有 bitflags 1/2(png 旧链)、
getrandom 0.2/0.4。单项都小(合计 ~100–300KiB),但方向应统一:
**引擎 crate 不直接依赖 sha2/walkdir,统一走 agenterm-script-common 的
`hex::sha256_hex` / `corpus_scan`**。
- 已落地:agenterm-qjs 的 sha2/walkdir 直接依赖实为未使用,已删除;tempfile 降为
  dev-dependency(本文同一提交)。
- 待各自 lane 处理:agenterm-lua 同样声明了 sha2 0.10,使用面待查;script-common
  自身升 sha2 0.11 后 0.10 链即可整条消失。

### P4 dist 语义:让"看到的体积"就是"承诺的体积"
本地 `stage-build dev` 把 debug 产物放进 dist,是这次 15MB 误会的来源。dist 清单
已如实记录 profile,不算错;但若希望 dist 恒代表可分发形态,本地默认链改
release-fast 即可(代价是本地迭代变慢,需权衡,不急)。

### P5 自有代码 1.7MiB 的复用长线
src/ 里最大的源文件:`platform/adapters/windows/remote_frontend.rs`(378KB)、
`platform/adapters/unix/frontend/mod.rs`(276KB)+ `render.rs`(132KB)、
`client/mod.rs`(157KB)。Win/Unix 两套 adapter 的渲染/快照逻辑存在多少可下沉到共享
frontend core 的重复,值得单独一轮 platform-ux-parity 视角的审计——这是"抽象与复用"
真正的长期标的,收益不止体积(平价缺陷会同源消失)。

## 3. 边界与不做的事

- rhai 不拆:rh 是任务系统与构建管线的承重墙。
- 不引入 wrapper/.com/动态库拆分来"作弊"减重:与单 PE 设计决定冲突。
- debug 产物体积不做目标。
- (2026-08-09 预算裁决)GUI 体积预算已由 4MiB 上调至 10MiB——体积不再是驱动;
  本文档自此以"持续的抽象与复用"为主目标,体积数据保留作事实基线。

## 4. 复用工作日志与队列(滚动更新)

### 已落地
- 2026-08-09 `03412921`:agenterm-qjs 未使用的 sha2/walkdir 直接依赖删除;tempfile
  降为 dev-dep(P3 第一刀)。
- 2026-08-09 `5e2936d8`:qjs/sql 的 check-many / corpus-scan **整命令体**下沉到
  script-common(继 parse_check_many_cli 下沉后的上一层)。两引擎各留 3–8 行适配,
  输出与退出码逐字节不变;crate 测试 common 47 / qjs 93 / sql 19 全绿。
- 同日并行 lane 的 `82019aa9` 开始退役独立引擎 exe——与本文档 P1/P2 的
  "引擎搭载策略收敛到主 PE + 编译期门控"同向,后续按其波次推进后再评估 feature 门。
- 2026-08-09(下半日,frontend 路线):`8e0766ba` 快照键集护栏;`6836dbca`
  ServerContextMenuRects 命名字段(消元组反转);`246e9c4f` F7 关闭
  (ControlWindow::control_selection + 共享 UTF-16→字符换算);`63ad5498`
  SidebarViewport(滚动模型半区,行命中半区留队列)。
- 2026-08-09(rh 语言/工具链):`027f8dd8` stderr_inherit 三层落地(build 实时输出);
  `867dbab1` prune 双修(PathBuf::from 变量克隆 + Windows POSIX 锁探测诚实跳过,
  build.bat dev 本机首次全绿);`457457bc` JSON 标量串化对齐解释器(语言层关闭
  `0 +` 强转 bug 类);`a088b99f` json==json 真 Value 等值(null 安全);后续一刀
  null→"" 判空成语对齐。CI 修复弧:`c3863f5c` nativecore 跨平台 fail-closed、
  `557b3f37`/`14592129` clippy 门、`3bbb05a7` 灰度全量处置、`64a05e6a` 门 deadline
  容纳冷 AOT 编译。

### 队列(按价值排序)
1. **P5 frontend 三面重复测绘**:已完成 → `plan/design-frontend-shared-core.md`。
   66 个同名函数横跨两 controller;五大提取候选(快照装配 ~600 行、选区生命周期
   ~450 行、modal 几何 ~500 行且已实际漂移、sidebar 命中 ~300 行、滚动条+指针合成
   ~450 行),另有 4 个可独立修的具体缺陷。下一步:先补"快照键集对等"护栏测试。
2. corpus-scan 契约测试 ×3(lua/qjs/sql 各 ~50 行结构相同)→ script-common
   test-support;顺带把"契约"从复制粘贴变成单点定义。
3. lua 的 corpus-scan/check-many 是否向共享命令体对齐:其"`--dir` 悬空回退 CWD"
   与人类输出格式为真实分叉,由 parity 测试钉住——对齐是产品决定,归 script lane。
4. sha2 0.10→0.11 统一(script-common 升版后 0.10 整链消失),连带 lua 的直接依赖
   使用面核查。

### 显式拒绝(记录以免反复)
- `read_source` 7 行 ×2(qjs/sql):不下沉。两份五行函数配各自文档注释的清晰度
  高于一个带错误映射参数的共享函数;不是所有重复都值得一个抽象。

## 5. 体积追踪与死代码灰度归档(2026-08-09 起)

### 5.1 体积追踪设施
- **逐产物历史**:stage-build 每次运行向 `dist/size-history.jsonl` 追加一行
  (commit、profile、各产物字节)。本地未跟踪文件——每台机器一条趋势线,
  避免共享 checkout 的追加冲突。
- **逐 crate 归因**:`scripts/rh/size-attribution.rh`(rh run 直跑)在专用
  `target/size-report` 目录用 cargo-bloat(--message-format json)产出
  `dist/size-attribution.json` 并打印 top-N 表。依赖 `cargo install cargo-bloat`,
  未安装时给出明确提示而非静默跳过。脚本接受 con 专属的
  `con-release-fast` / `con-release` profile，并为 `agenterm-con` 显式选择
  workspace package；con 的 strip=none 归因样本使用 host std，官方 staging
  另走 custom-std，因此 crate 排名用于选刀，样本文件大小不作为发布证据。
- 注意:dist 里 dev 与 release-fast 产物会被不同 lane 轮流暂存,**对比体积必须
  先对齐 profile**(size-history 每行都带 profile 字段,别跨行直接比)。

2026-08-12 的 con-release-fast 首个可归因样本给出 `.text` 排名：`std`
160,329 B、`agenterm_con` 140,160 B、`agenterm_platform` 98,046 B、`vt100`
16,901 B、`agenterm_ui_core` 4,178 B。最大单函数是 control dispatch 20,994 B，
其次是主事件分派 15,693 B；CLI run/parse 合计 12,643 B。正式 strip+custom-std
x64 PE 同期仍为 619,520 B（`.text` 404,348 B），不得把归因样本的 434,200 B
汇总值与正式节区直接相减。该证据否决继续优先微调像素 ISA，下一轮先审计
control/CLI 的重复状态机和错误格式化。

同日三项最终 PE 淘汰实验不得重复凭源码直觉重做：把 `SendMouse` 与
`SendWheel` 的 target/cell 校验抽成具体函数只令 `.text -32 B`、`.rdata
+16 B`，619 KiB 对齐后的文件不变；以 21 字节栈缓冲替换七种 JSON 整数
`ToString` 时 `.text` 不变、`.rdata +88 B`、`.reloc +12 B`；给 Win32
`window_proc_inner` 加 `inline(never)` 后所有节区完全不变，证明 LLVM 原本
就未内联它，bloat 把 6,946 B 记到 unwind thunk 只是归因边界。三者均已
回退。只有能删除仍未被其它调用保留的运行时族、或跨过最终文件对齐边界
的候选才进入实现。

随后把 list/new/select/close 的六处稳定 `@TAB_ID` JSON 表示集中到一个
非泛型、非内联的 `Option<TabId> -> JsonValue` 边界。`map_or` 版本净减 384
节区字节但文件不变；改为显式 `match` 后 `.text -576 B`、`.rdata -8 B`、
`.pdata -12 B`，release-fast PE 从 616,448 降至 615,936 B。继续把 helper
下沉为手写栈十进制却使 PE 增长 512 B，故回到集中 `format!`。结论是先消除
重复所有权/closure 状态机，再让已链接的标准格式化完成叶子工作；“更接近
汇编”不是独立收益指标。

x86 feature detection 也完成了全链实验后回退：UI-core 与 platform 的 8 处
生产 `is_x86_feature_detected!` 曾全部替换为 CPUID/XGETBV（含 XSAVE、
OSXSAVE、AVX、XCR0[1:2]、AVX2、SSSE3、FMA 条件），test-only oracle 与
Rust 标准检测逐位一致。但带符号 con 图中的
`std_detect::detect::cache::detect_and_initialize` 仍完整保留 1,688 B，说明
另一个标准/第三方依赖仍拥有该运行时；新增两份 raw detector 使正式 PE
增长 512 B、有效节区增长 83 B。实现已回退。只有先证明依赖图最后一个
std_detect owner 可删除，才重开 CPUID/汇编替换，不能以仓库搜索零命中代替
最终链接证据。

### 5.2 死代码灰度归档流程
编译器已经在持续报告死代码——先把信号清单化,再按"冷却期"分级处置,
不在活跃 lane 的热文件上直接动刀:
1. **清单化**:每轮 loop 用 `cargo build --workspace` 收集 dead_code/unused 警告,
   更新下表(新增/消失都记)。
2. **归属与冷却**:每项标注疑似归属 lane;连续 ≥2 天仍在清单上且其文件无
   in-flight 改动(git status 干净)才进入处置。
3. **处置**:优先删除(git 历史即归档);语义上"未来会接线"的,要求归属 lane
   加 `#[expect(dead_code, reason)]` 注明意图,否则按死代码删。
4. 长线:清零后在 CI 把 dead_code 升级为 deny,防再堆积。

### 5.3 清单(2026-08-09 首采 13 项;同日 CI clippy 红触发全量处置)
处置原则的首次全量执行:CI `-D warnings` 蔓延到主 crate 后冷却期即时终结。
删除 = 无消费者且无在制迹象;`#[expect(dead_code, reason)]` = 疑似在制接线,
注明到期删除条件。

| 位置 | 符号 | 处置 |
|---|---|---|
| crates/agenterm-rh/transpile.rs:134 | `emit_scope_json_expr` | `#[expect]`(AOT 可能在接线) |
| src/client/mod.rs:5 | `use BufRead` | 删除 |
| src/platform/adapters/unix/frontend/mod.rs:51 | `TerminalAppearanceOverride` | 删除 |
| src/platform/mod.rs:50 | `ConsoleKey`/`LineBuffer`/`LineHistory` 导入 | 删除(facade 收窄为 `ConsoleLineEditor`) |
| src/platform/mod.rs:64 | `enter_console_line_editor` | `#[expect]`(console-line-editor 产品接线在制) |
| src/script_rh_host.rs:10 | `RhHostEntryValue::{Unit,Value}` | `#[expect]`(typed entry-value 通道待接) |
| src/script_lua_run.rs:75 | `current_run_context` | `#[expect]`(lua 消费者未接) |
| src/script_worker.rs | `classify_runtime_error` 死链(含 4 个构造器与其专属测试) | **删除**——retirement 孤儿,hosted 引擎已走类型化失败;token 表在 git 历史 |
| src/frontend/server_strip_ui.rs:37 | `StripRect::width` | `#[cfg_attr(not(test), expect)]`(仅测试消费) |
| src/script_rh_host.rs:229(顺带) | `host_process_request` 复杂返回元组 | 命名为 `type ProcessRequest`(clippy type_complexity) |
| scripts/rh/artifact-verification.rh:185 | 探测已删除的 `dist\agenterm-cli.exe` | **待修**(release 通道必炸,归 artifact 合同 lane) |

首采当日的 release-fast 归因快照(size-attribution.rh 产出,strip=none):
.text 8.13MiB — agenterm 22.4%、C 代码(LuaJIT+QuickJS+SQLite)19.8%、sqlparser
19.6%、rhai 11.2%、std 6.6%。较上一次测量的显著变化:rusqlite/SQLite 的加入把
无名 C 行从 0.76MiB 抬到 1.62MiB(sql M1 的代价,预算内)。

### 2026-08-12: static font catalogs and assembly leaf threshold

- Platform font candidates are immutable build-time catalogs. Expose them as
  `&'static [FontFileCandidate]`; keep `Vec` only for runtime discovery results.
  This removes false ownership and Unix renderer-initialization allocations.
- A Win64 `global_asm!` GDI gray8-to-alpha leaf passed its owning test but grew
  the staged con PE from 615,936 B to 616,448 B. The compiler already optimized
  the bounded Rust loop well; the extra ABI boundary and validation helper cost
  one 512-byte PE alignment unit. The experiment was fully reverted.
- Size-sensitive assembly is accepted only when final staged bytes shrink or a
  separately measured hot-path gain justifies the exact cost. Instruction-count
  intuition is not evidence, and moving code behind an FFI symbol is not removal.
