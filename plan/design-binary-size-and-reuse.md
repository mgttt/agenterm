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
