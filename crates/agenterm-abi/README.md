# agenterm-abi (libagenterm)

C ABI 导出壳：嵌入方（agenterm / agenterm-con / agenterm-cu）与 OS 之间的
**机制**边界。仅导出 `exports.txt` 中的 `agt_*` 符号，不含产品概念。

里程碑：1 = 版本/错误/能力；2 = PTY（`agt_pty_*`）；3a = 窗口生命周期与
帧会合（`agt_window_open/poll_event/request_redraw/metrics/close` +
`agt_frame_begin/commit`）。事件翻译在 3a 只做 4 种（close / geometry /
focus / render_due），键盘/指针/滚轮/IME 留给 3b。4 = 截图；5 = 进程枚举 /
kill / self pid；6 = 结构化 accessibility-tree 观察与节点动作（`agt_a11y_*`：
扁平树快照、节点字段、按路径 click/focus）。主机 accessibility 栈（Windows
UIA / macOS AX / Linux AT-SPI2）藏在 `agenterm-platform` 适配器后，C 头文件只
描述机制。

## 构建（必须用 unwind profile）

规格 §3.8：panic 不得穿过 FFI 边界——每个导出都包了 `catch_unwind`，
这要求 `panic = "unwind"`。工作区默认 `[profile.dev]` / `[profile.release]`
均为 `panic = "abort"`，因此本 crate 显式使用专用 unwind profile；
在 abort profile 下编译会触发 `src/lib.rs` 顶部的 `compile_error!` 闸而失败
——这是预期信号，不是可以绕过的警告。

```powershell
# 交付 cdylib（release 语义，panic=unwind）→ target/abi-release/
cargo build -p agenterm-abi --profile abi-release

# 开发 / 测试（panic=unwind；同时构建 cdylib 并运行全部测试）
cargo test -p agenterm-abi --profile abi-dev

# 格式化检查（CI 闸：全 workspace 必须干净，退出码 0）
cargo fmt --all -- --check
```

任何不带 `--profile abi-*` 的 `cargo build/test -p agenterm-abi` 都会因编译期
闸失败（默认 profile 是 abort，会静默产出无围栏的库）。

## 产物形态

`[lib] crate-type = ["cdylib", "staticlib", "rlib"]`，一次构建产出三类文件：

| 形态 | Windows | Unix（Linux/macOS） | 适用场景 |
|------|---------|---------------------|----------|
| 动态库 `cdylib` | `agenterm.dll`（+ 导入库 `agenterm.dll.lib`） | `libagenterm.so` / `libagenterm.dylib` | C 消费者常规交付：运行时加载，升级只需替换库文件 |
| 静态库 `staticlib` | `agenterm.lib` | `libagenterm.a` | C 消费者嵌入场景：链接进可执行文件，不想携带动态库文件 |
| Rust 库 `rlib` | `libagenterm.rlib` | `libagenterm.rlib` | 进程内 Rust 消费者（`agenterm-cu`）直接 `use agenterm::`，无需 dlopen |

三者均位于 `target/<profile>/`（profile 为 `abi-dev` 或 `abi-release`）。

**静态库与动态库导出同一批 39 个 `agt_*` 符号**（`exports.txt` 为准，
`tests/exports_set.rs` 与 `tests/artifacts.rs` 分别闸住符号集与产物存在性）。

**静态链接时 panic 围栏同样要求 `panic = "unwind"`**：静态库仍必须用
`--profile abi-release` / `abi-dev` 构建，默认 `dev` / `release`（abort）
会被 `compile_error!` 闸挡住。除非开启 `allow-abort-profile`——但那样
构建出的库没有 `catch_unwind` 围栏，只适合没有 C 边界的 Rust 内部消费者。

> **命名**：产物文件名现在是 `libagenterm.{a,so,dylib}` / `agenterm.dll`，
> 与 `plan/plan-v0.1.18.md` §14 一致（里程碑 17 完成改名）。**package 名仍是
> `agenterm-abi`**（Cargo 依赖声明用它）；**lib/crate 名是 `agenterm`**（Rust
> `use agenterm::` 与产物文件名用它）。

## `allow-abort-profile` feature（逃生舱，默认关闭）

该 feature 是给**没有 C 边界的 Rust 原生 rlib 消费者**（如 `agenterm-cu`
静态链接本 crate）用的：它们不需要 `catch_unwind` 围栏，panic 在 Rust 内部
正常传播，`panic=abort` 是合法选择。开它 = **放弃 panic 围栏**——abort
profile 下构建出的库没有任何 `catch_unwind` 保护，**只允许**这类纯 Rust
内部消费者使用。

**交付 cdylib 的路径永远不开这个 feature**：C 消费者跨 FFI 边界，panic
必须被 `catch_unwind` 拦成 `AGT_FAILED { code = "panic" }`，因此交付构建
必须继续走 `--profile abi-release` / `abi-dev`（unwind）。

## ABI 版本

`agt_abi_version()` 返回 `(major << 16) | minor`（见 `include/agenterm.h`
的 `AGT_ABI_MAJOR` / `AGT_ABI_MINOR` / `AGT_ABI_VERSION` 宏）。规则：

- **major**：只在**破坏性变更**时递增——改签名、删符号、改语义。
  消费者必须拒绝不匹配的 major（`v >> 16 != AGT_ABI_MAJOR` 即视为不兼容）。
- **minor**：**新增导出**时递增（新增机制、纯增量），老消费者不受影响，
  无需重新编译。

当前为 `0x00010001`（major=1, minor=1）：里程碑 2–10 陆续新增了
PTY / window / frame / input / screenshot / process / clipboard /
parent-console / runtime / a11y 等大量向后兼容导出，minor 随导出面增长
从 0 递增到 1。

`agt_build_id()` 返回 `<crate 版本>+abi.<major>.<minor>`
（例如 `0.1.16+abi.1.1`），在**编译期**由 `CARGO_PKG_VERSION` 与
`ABI_MAJOR` / `ABI_MINOR` 常量拼接而成——不手写字面量，crate 版本或 ABI
常量一改，build id 自动跟随，不会过期。字符串以 NUL 结尾、静态、永久有效。

## 测试

- `tests/exports_set.rs`：导出符号集与 `exports.txt` 完全一致（编译期不改 ABI）。
- `tests/dylib_load.rs`：用 `libloading` 加载真实 cdylib，调用导出并断言
  返回的 `const char*` 均为合法 NUL 结尾 C 字符串（缺陷回归闸）。找不到
  cdylib 时该测试直接失败（先执行上面的 build 命令）。

## 已知平台限制

窗口循环线程模型（库内私有线程跑 `run_pixel_window`）目前只在 **Windows**
验证（消息泵归创建线程）。**macOS** 的 AppKit 要求窗口/事件循环在主线程，
需主线程宿主，留待后续里程碑。`include/agenterm.h` 亦写明此限制。

`agenterm-cu` 的 `tree` / 结构化 `click` / `focus` 在 Linux `current` 上经
本 crate 的 `agt_a11y_*` 机制消费；`windows` / `screenshot` / 坐标降级输入仍
直连 `agenterm-platform`，待对应 ABI 里程碑落地后迁入。
