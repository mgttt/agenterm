# size-probe — 静态 vs 动态两变体尺寸探针（里程碑 15 + 23 + 34）

> 目的：在把 `agenterm-con` 迁到 dylib 消费之前，先用一个小程序测出"消费者迁移后
> 能瘦多少"（记作 `S`）的量级，据此提前判决 Phase 0 判据 2（共享收益）是否可能成立。
> 这是"先做能出判决的便宜实验"：结论是**估算**，不是 con 迁移的实测替代品。
>
> 里程碑 15 只覆盖 runtime / clipboard / process / parent-console 四组机制（`S_probe`
> = −14,848 B，不足以判决）；**里程碑 23 把 window_host 与 pty —— con 里最大的两块
> 机制 —— 补进探针**（`S_probe` = +87,040 B，仍差 46,592 B）；**里程碑 34 把剩余导出
> 也接进探针**（screenshot 双导出 / a11y / input event_text 路径），把"剩余机制补不上
> 缺口"从推断变成实测（本轮 `S_probe` = +87,040 B，结论见下文）。

基线（`plan/phase0-baseline-measurements.md` 实测）：`libagenterm.dll` = **400,896 B**，
三个消费者**每个平均要瘦掉 > 133,632 B**（= 400,896 / 3）共享才真的省字节。
（本轮实测 `libagenterm.dll` = 403,456 B，与基线差 2,560 B，属构建环境差异；阈值对照
沿用 133,632 B 以便与 M15/M23 同口径可比。）

## 两个变体

| 变体 | 获取机制的途径 | 依赖 |
|------|----------------|------|
| **A（静态）** | 直接调 `agenterm-platform` 的 Rust API（rlib 静态链接） | `agenterm-platform`（features 照抄 `agenterm-abi/Cargo.toml`：`pty`, `native-pixel-window`, `portable-pixel-window`, `input`, `ime`, `screenshot`, `process`, `clipboard`, `parent-console`, `a11y-tree`, `runtime`） |
| **B（动态）** | 只经 `libagenterm` cdylib 的 C 导出（`libloading` 运行期 `dlopen`/`LoadLibrary`） | 仅 `libloading`（+ std） |

两个变体做同一组事（都真实调用到，不是编译进去不用）：

1. 取用户配置目录的长度（不打印内容）
2. 取默认 shell 的长度（不打印内容）
3. 查一次剪贴板是否有 Unicode 文本（只打印布尔）
4. 查一次进程列表所需条数（两段式探测）
5. 往父控制台写一行短文本
6. 打印 `abi_version`（B 用导出；A 无此概念，打印常量占位）
7. **window（里程碑 23）**：调一次窗口入口（A 走 `run_pixel_window` 且 `opened()` 后立即
   `Exit`；B 走 `agt_window_open` 成功后 `agt_window_close`）。窗口在无头环境/macOS 上
   打不开**完全可以接受**——关键是符号被引用、代码被链接；`AGT_UNSUPPORTED`/失败只打印
   状态，不使探针失败
8. **pty（里程碑 23）**：起一个最短命的子进程（Windows `cmd.exe /c exit`，Unix
   `/bin/sh -c exit`）并立刻收掉（A 走 `shutdown_session_detached`；B 走
   `agt_pty_open` → `agt_pty_wait` → `agt_pty_close`）。失败同样可接受，打印状态即可
9. **screenshot 编码（里程碑 34）**：把 1×1 帧编码成 PNG 写到 `std::env::temp_dir()`
   再删除——**绝不写仓库树**（A 走 `write_xrgb_png`；B 走 `agt_screenshot_write_png`）。
   编码成功是本轮实测预期
10. **screenshot 窗口捕获（里程碑 34）**：B 走 `agt_screenshot_capture_window` 传
    `native_window = 0`（触发 `bad_handle` 参数校验，不产生文件）；A 走
    `capture_native_window_png` 传 dummy 非零句柄（在 Windows 后端参数校验阶段即失败，
    无头/macOS 上为 Unsupported）。失败可接受，打印状态即可
11. **a11y（里程碑 34）**：取一次无障碍树（A 走 `tree_for_window(None)`；B 走
    `agt_a11y_tree_snapshot(0, …)`）。Windows/macOS 上为 Unsupported **完全可以接受**
12. **input event_text 路径（里程碑 34）**：B 在 `agt_window_open` 成功后在活句柄上调
    `agt_window_event_text`（两段式，只打印字节长度）；A 在窗口应用里显式匹配
    `PixelWindowEvent::Ime` 事件（只统计文本长度）。文本内容一律不打印

> 里程碑 15 明确**没有覆盖 window_host 与 pty**，因此当时 `S_probe` 是**下界估算**、
> 只能给出第三档"不足以判决"。里程碑 23 补上这两块后，`S_probe` 逼近真实 con 的 `S`，
> 但当时"剩余机制补不上缺口"是**推断**。里程碑 34 把剩余导出（screenshot / a11y /
> event_text）实测进探针，把这一步从推断变成实测（结论见下文）。

## 构建与运行（Windows 实测；Unix 产物名去掉 `.exe`）

前置：先构建 libagenterm cdylib（变体 B 运行期加载它）：

```
cargo build -p agenterm-abi --profile abi-release
```

两个变体用**同一个 profile（`--release`，同一 target）**构建：

```
cargo build -p size-probe --release
```

运行：

```
./target/release/size-probe-variant-a-static.exe
./target/release/size-probe-variant-b-dynamic.exe
```

变体 B 的 cdylib 查找顺序：`AGENTERM_ABI_LIB` 环境变量 → 从可执行文件向上
在候选 profile 目录（`abi-release/`、`abi-dev/`、`release/`、`debug/`）中找
`agenterm.dll` / `libagenterm.so` / `libagenterm.dylib`。

## 实测结果

测量环境：Windows，`x86_64-pc-windows-msvc`，仓库 release profile
（`opt-level="z"`、`lto="thin"`、`codegen-units=1`、`strip=true`、`panic="abort"`）。
字节数取自构建命令退出后对产物的实际 `stat -c %s`。

### 里程碑 34（全量：+ screenshot 双导出 + a11y + event_text）——本轮口径

| 产物 | 构建命令 | 字节数 |
|------|----------|--------|
| 变体 A（静态） | `cargo build -p size-probe --release` → `target/release/size-probe-variant-a-static.exe` | **251,392** |
| 变体 B（动态） | 同上（同一命令）→ `target/release/size-probe-variant-b-dynamic.exe` | **164,352** |
| （前置）libagenterm | `cargo build -p agenterm-abi --profile abi-release` → `target/abi-release/agenterm.dll` | 403,456（本轮实测；基线 400,896） |

```
S_probe = sizeof(变体A) - sizeof(变体B) = 251,392 - 164,352 = +87,040 B
```

### 里程碑 23（6 组机制，含 window + pty）——旧口径对照

| 产物 | 字节数 |
|------|--------|
| 变体 A（静态） | 238,592 |
| 变体 B（动态） | 151,552 |

```
S_probe（M23 口径）= 238,592 - 151,552 = +87,040 B
```

### 里程碑 15（4 组机制）——旧口径对照

| 产物 | 字节数 |
|------|--------|
| 变体 A（静态） | 132,608 |
| 变体 B（动态） | 147,456 |

```
S_probe（旧口径）= 132,608 - 147,456 = -14,848 B
```

window+pty 两块的增量贡献（同为 `--release` 实测，可比）：

- 变体 A：238,592 − 132,608 = **+105,984 B**（window+pty 机制字节经 thin LTO 后的静态成本）
- 变体 B：151,552 − 147,456 = **+4,096 B**（仅 window/pty 的 FFI 胶水与符号解析）
- `S_probe` 增量：87,040 − (−14,848) = **+101,888 B**

**剩余机制（screenshot / a11y / event_text）的实测贡献**（M34 − M23，同为 `--release`）：

- 变体 A：251,392 − 238,592 = **+12,800 B**（截图编码 + 窗口捕获后端 + a11y stub + IME 匹配的静态成本）
- 变体 B：164,352 − 151,552 = **+12,800 B**（新增导出解析与探测胶水）
- `S_probe` 增量：87,040 − 87,040 = **+0 B**

也就是说：**剩余机制合计没有给 `S_probe` 带来任何提升**——变体 A 的机制静态成本恰好被
变体 B 的胶水增量抵消（两者同为 +12,800 B，对齐到 PE 512 B 边界后逐字节相等）。

PE 导入表佐证（`dumpbin /imports`）：里程碑 23 的变体 A 额外导入了
`CreatePseudoConsole`/`Conpty*`（kernel32）与 `CreateWindowExW`（user32）等，
机制代码确实进入了 A 的产物；变体 B 仍无这些静态导入，机制只经运行期 `LoadLibrary`。
里程碑 34 的佐证：变体 A 的产物中含有截图错误码字符串（`screenshot_too_large`、
`screenshot_invalid_bounds`，来自 `adapters/windows/ui_screenshot.rs`），变体 B 中
搜不到——截图机制代码进入了 A、全部留在 dylib。

### 与阈值 133,632 B 的对照

**小于（但仍为正值）**。`S_probe` = +87,040 B 自里程碑 23 起**没有变化**：
剩余机制（screenshot 双导出 / a11y / input event_text）对 `S_probe` 的贡献实测为
**+0 B**，缺口**仍是 46,592 B**（`S_probe` 仅为阈值的 65%）。

### 结论（三档之一）

**「档 2：判据 2 很可能不成立」**

- `S_probe`（+87,040 B）**显著小于**阈值 133,632 B，缺口 46,592 B；
- **明确回答本轮的问题**：剩余机制实际贡献了 **+0 B**（A/B 同增 12,800 B，恰好抵消），
  **没有补上 46,592 B 的缺口**。上一轮"剩余机制补不上缺口"的推断现已是**实测**：
  screenshot 的 PNG 编码 + 窗口捕获后端、a11y、input event_text 路径全部接进探针后，
  `S_probe` 一分未涨——它们的静态成本与变体 B 的 FFI 胶水成本相当，对共享收益毫无贡献；
- 关键证据链：con 中公认最大的两块（window+pty）为 `S_probe` 贡献约 101,888 B，
  **剩余全部机制合计贡献 0 B**。要让判据 2 成立，还需要 > 46,592 B，而"剩余机制"这
  条路已实测走不通；filesystem / ipc 等在 con 中的体积占比更小于 screenshot 这类
  自带编码库的机制，翻盘空间可以忽略；
- 因此"共享 libagenterm 真省字节"（Phase 0 判据 2）**很可能不成立**，而且是
  **不需要先迁 con 就能拿到的负面结论**；本 README 如实记录数字，未改动 `plan/**`。

**已排除的结论**：档 1（`S_probe` > 阈值）不成立；档 3（不足以判决）被排除——
里程碑 15 的"不足以判决"正是因为 window+pty 未实测；里程碑 23 补上后剩余机制仍是推断；
里程碑 34 把剩余机制实测后，`S_probe` 依旧停在 +87,040 B，三档里只剩档 2。

> 提示：若后续想进一步确认，可在真实 con 迁移实测时复核本估算；但按当前证据，
> 判据 2 的前景不乐观，继续投入前应先在 `plan/` 层决策。

## 运行输出示例（本机实测，里程碑 34）

变体 A：

```
size-probe variant A (static: agenterm-platform rlib)
size-probe[variant A] parent-console write ok
user_config_dir_len=32
default_shell_len=27
clipboard_has_text=true
process_count=382
parent_console_write_stdout=ok
window_open=ok
pty_open=ok
screenshot=ok(pixels=1)
screenshot_capture=failed(screenshot_invalid_bounds)
a11y=unsupported
event_text_len=0
abi_version=0(static placeholder)
```

变体 B：

```
size-probe variant B (dynamic: libagenterm cdylib via libloading)
size-probe[variant B] parent-console write ok
user_config_dir_len=32
default_shell_len=27
clipboard_has_text=true
process_count=381
parent_console_write_stdout=ok
window_open=ok
pty_open=ok(exit_code=0)
screenshot=ok(pixels=1)
screenshot_capture=failed(status=2)
a11y=unsupported
event_text_len=len=0
abi_version=65539
```

两边各项数值一致（进程数是瞬态值，采样瞬间可能差一）；只打印长度/布尔/状态码/数字，
不打印路径、剪贴板内容或 IME 文本。`window_open`/`pty_open` 在真实桌面上均为 `ok`；
`screenshot` 编码两个变体都真实成功（temp 文件用完即删）；`screenshot_capture` 按设计
走参数校验失败路径（A 为 `screenshot_invalid_bounds`，B 为 `status=2`/`AGT_FAILED`）；
`a11y=unsupported` 是 Windows 上该机制的预期状态（机制在 dylib/stub 中，探针只关心
链接与路由）；`event_text_len=0`/`len=0` 表示窗口生命周期内没有 IME 文本。无头
CI / macOS 上允许 `window_open`/`pty_open`/`a11y` 为 `unsupported`/`failed`（探针
不因此失败）。`abi_version=65539` = `(1 << 16) | 3`（ABI 1.3）。

### 变体 B 的 pty 调用约定（调试留档）

`agt_pty_open` 按 ABI 约定 `argv[0]` 是程序名、`argv[1..argc]` 才是参数（内部取
`argv[1..]` 拼给 `ChildCommand`）。因此 Windows 下要 spawn `cmd.exe /c exit` 必须传
`argv = ["cmd.exe", "/c", "exit"]`、`argc = 3`；若漏掉 `argv[0]`（如只传
`["/c", "exit"]`），内部实际执行的是 `cmd.exe exit`（无 `/c`），cmd 不会退出，
`agt_pty_wait` 会超时（`error_code=timeout`）。这与 `crates/agenterm-abi/tests/dylib_load.rs`
的用法一致。
