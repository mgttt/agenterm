# rh — 现在是什么

SSOT：`plan/design-rh-standalone-product.md`。本文只陈述现状，不记过程。

**rh** 是一门默认解释执行的动态语言：私仓 `partnernetsoftware/rh`（CLI / loader / `rh-lang`），AgenTerm 仍 path 依赖工作树里的 crate，公开之前不 git-pin。

| | |
|---|---|
| 执行 | 树遍历 Language 1。无 rustc。`Engine::compile` 留缝，默认 `Unsupported`。 |
| 强大 | Host、诚实报错、`check` 不撒谎、沙箱不炸宿主、语料。不是 JIT/AOT，不是比 bun 快。 |
| 包装 | `rh.com` 只做 loader。六格原生切片。解开包再跑。交叉包整包都是目标格。 |
| 嵌入 | `Engine` + `Host` + `Value`，与 CLI 同级。AgenTerm `eval`/`run` 已走解释器；`pack`/`qualify`/`task` 仍 AOT。 |
| iOS | 解释器签进宿主 App，`.rh` 是数据。不出码、不加第七格、不上 libtcc。 |
| 桌面以后 | 同一语言底下才许 JIT/AOT。iOS 不许。 |
| 冻结不做 | 公开仓、crates.io、REPL、f64、默认 FFI、HTTP、闭包、Cosmopolitan libc。 |

`check` 扫每个 `std::` / `rh::`。点号形式（`app.tabs.list`）要等运行期：根名字未绑定才是 host。误拒合法程序比漏检嵌入方拼写更糟。

验收：`rh/crates/rh-lang/tests/accept/`。缺口清单：`plan/rh-tdd-review.md`。工作目标：`plan/goal-rh-product.md`。
