# size-probe — 静态 vs 动态两变体尺寸探针（里程碑 15 + 23）

> 目的：在把 `agenterm-con` 迁到 dylib 消费之前，先用一个小程序测出"消费者迁移后
> 能瘦多少"（记作 `S`）的量级，据此提前判决 Phase 0 判据 2（共享收益）是否可能成立。
> 这是"先做能出判决的便宜实验"：结论是**估算**，不是 con 迁移的实测替代品。
>
> 里程碑 15 只覆盖 runtime / clipboard / process / parent-console 四组机制（`S_probe`
> = −14,848 B，不足以判决）；**里程碑 23 把 window_host 与 pty —— con 里最大的两块
> 机制 —— 补进探针**，让 `S_probe` 逼近真实值（`S_probe` = +87,040 B，结论见下文）。

基线（`plan/phase0-baseline-measurements.md` 实测）：`libagenterm.dll` = **400,896 B**，
三个消费者**每个平均要瘦掉 > 133,632 B**（= 400,896 / 3）共享才真的省字节。

## 两个变体

| 变体 | 获取机制的途径 | 依赖 |
|------|----------------|------|
| **A（静态）** | 直接调 `agenterm-platform` 的 Rust API（rlib 静态链接） | `agenterm-platform`（features: `process`, `clipboard`, `pty`, `native-pixel-window`, `input`, `ime`） |
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

> 里程碑 15 明确**没有覆盖 window_host 与 pty**，因此当时 `S_probe` 是**下界估算**、
> 只能给出第三档"不足以判决"。里程碑 23 补上这两块后，`S_probe` 已逼近真实 con 的 `S`：
> 未覆盖的剩余机制（screenshot / a11y / filesystem / ipc 等）在 con 中的体积占比远小于
> window+pty，正文结论据此给出。

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

### 里程碑 23（6 组机制，含 window + pty）——本轮口径

| 产物 | 构建命令 | 字节数 |
|------|----------|--------|
| 变体 A（静态） | `cargo build -p size-probe --release` → `target/release/size-probe-variant-a-static.exe` | **238,592** |
| 变体 B（动态） | 同上（同一命令）→ `target/release/size-probe-variant-b-dynamic.exe` | **151,552** |
| （前置）libagenterm | `cargo build -p agenterm-abi --profile abi-release` → `target/abi-release/agenterm.dll` | 400,896（与基线一致） |

```
S_probe = sizeof(变体A) - sizeof(变体B) = 238,592 - 151,552 = +87,040 B
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

PE 导入表佐证（`dumpbin /imports`）：里程碑 23 的变体 A 额外导入了
`CreatePseudoConsole`/`Conpty*`（kernel32）与 `CreateWindowExW`（user32）等，
机制代码确实进入了 A 的产物；变体 B 仍无这些静态导入，机制只经运行期 `LoadLibrary`。

### 与阈值 133,632 B 的对照

**小于（但仍为正值）**。里程碑 23 把 con 里最大的两块机制补进来后，
`S_probe` = +87,040 B，比里程碑 15（−14,848 B）提高了 101,888 B，
但**仍比盈亏平衡阈值 133,632 B 低 46,592 B**（仅为阈值的 65%）。

### 结论（三档之一）

**「档 2：判据 2 很可能不成立」**

- `S_probe`（+87,040 B）**显著小于**阈值 133,632 B，缺口 46,592 B；
- 关键证据是**window+pty 已经实测**：这两块正是 con 中公认最大的机制（里程碑 15
  的全部希望都押在它们身上），合计才为 `S_probe` 贡献约 101,888 B——要让判据 2
  翻盘，**其余所有未覆盖机制（screenshot / a11y / filesystem / ipc / 其余 input/ime
  路径）合计还要贡献 > 46,592 B** 的静态成本，而它们在 con 中的体积占比远小于
  window+pty，这个缺口大概率补不上；
- 因此"共享 libagenterm 真省字节"（Phase 0 判据 2）**很可能不成立**，而且是
  **不需要先迁 con 就能拿到的负面结论**；本 README 如实记录数字，未改动 `plan/**`。

**已排除的结论**：档 1（`S_probe` > 阈值）不成立；档 3（不足以判决）被本轮排除——
里程碑 15 的"不足以判决"正是因为 window+pty 未实测，本轮已补上，且两者合计仍远小于
缺口，剩余机制翻盘空间很小。

> 提示：若后续想进一步确认，可在真实 con 迁移实测时复核本估算；但按当前证据，
> 判据 2 的前景不乐观，继续投入前应先在 `plan/` 层决策。

## 运行输出示例（本机实测，里程碑 23）

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
abi_version=0(static placeholder)
```

变体 B：

```
size-probe variant B (dynamic: libagenterm cdylib via libloading)
size-probe[variant B] parent-console write ok
user_config_dir_len=32
default_shell_len=27
clipboard_has_text=true
process_count=382
parent_console_write_stdout=ok
window_open=ok
pty_open=ok(exit_code=0)
abi_version=65537
```

两边八项数值一致（进程数是瞬态值，采样瞬间可能差一）；只打印长度/布尔/状态码/数字，
不打印路径或剪贴板内容。`window_open`/`pty_open` 在真实桌面上均为 `ok`；在无头
CI / macOS 上允许为 `unsupported`/`failed`（探针不因此失败）。`abi_version=65537` =
`(1 << 16) | 1`（ABI 1.1）。

### 变体 B 的 pty 调用约定（调试留档）

`agt_pty_open` 按 ABI 约定 `argv[0]` 是程序名、`argv[1..argc]` 才是参数（内部取
`argv[1..]` 拼给 `ChildCommand`）。因此 Windows 下要 spawn `cmd.exe /c exit` 必须传
`argv = ["cmd.exe", "/c", "exit"]`、`argc = 3`；若漏掉 `argv[0]`（如只传
`["/c", "exit"]`），内部实际执行的是 `cmd.exe exit`（无 `/c`），cmd 不会退出，
`agt_pty_wait` 会超时（`error_code=timeout`）。这与 `crates/agenterm-abi/tests/dylib_load.rs`
的用法一致。
