# size-probe — 静态 vs 动态两变体尺寸探针（里程碑 15）

> 目的：在把 `agenterm-con` 迁到 dylib 消费之前，先用一个小程序测出"消费者迁移后
> 能瘦多少"（记作 `S`）的量级，据此提前判决 Phase 0 判据 2（共享收益）是否可能成立。
> 这是"先做能出判决的便宜实验"：结论是**估算**，不是 con 迁移的实测替代品。

基线（`plan/phase0-baseline-measurements.md` 实测）：`libagenterm.dll` = **400,896 B**，
三个消费者**每个平均要瘦掉 > 133,632 B**（= 400,896 / 3）共享才真的省字节。

## 两个变体

| 变体 | 获取机制的途径 | 依赖 |
|------|----------------|------|
| **A（静态）** | 直接调 `agenterm-platform` 的 Rust API（rlib 静态链接） | `agenterm-platform`（features: `process`, `clipboard`） |
| **B（动态）** | 只经 `libagenterm` cdylib 的 C 导出（`libloading` 运行期 `dlopen`/`LoadLibrary`） | 仅 `libloading`（+ std） |

两个变体做同一组事（都真实调用到，不是编译进去不用）：

1. 取用户配置目录的长度（不打印内容）
2. 取默认 shell 的长度（不打印内容）
3. 查一次剪贴板是否有 Unicode 文本（只打印布尔）
4. 查一次进程列表所需条数（两段式探测）
5. 往父控制台写一行短文本
6. 打印 `abi_version`（B 用导出；A 无此概念，打印常量占位）

> **没有覆盖 window_host 与 pty 两块最大的机制**——探针不开窗口、不起 PTY、
> 不写截图、不 kill 进程（这些会引入不可控依赖）。因此 `S_probe` 是**下界估算**：
> 真实 con 的 `S` 很可能高于 `S_probe`。

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

| 产物 | 构建命令 | 字节数 |
|------|----------|--------|
| 变体 A（静态） | `cargo build -p size-probe --release` → `target/release/size-probe-variant-a-static.exe` | **132,608** |
| 变体 B（动态） | 同上（同一命令）→ `target/release/size-probe-variant-b-dynamic.exe` | **147,456** |
| （前置）libagenterm | `cargo build -p agenterm-abi --profile abi-release` → `target/abi-release/agenterm.dll` | 400,896（与基线一致） |

```
S_probe = sizeof(变体A) - sizeof(变体B) = 132,608 - 147,456 = -14,848 B
```

**注意 `S_probe` 是负的**：对探针覆盖的四类机制（runtime / clipboard / process /
parent-console），静态链接的机制代码经 thin LTO 裁剪后**几乎没有体积成本**——变体 A
甚至比带 `libloading` + 符号查找 + 两段式 FFI 胶水的变体 B 还小约 14.8 KB。
PE 导入表佐证：A 静态导入了 `shell32.dll` / `user32.dll`（机制代码确实进入 A 的产物），
B 没有（机制代码确实未静态进入 B，只经运行期 `LoadLibrary`）。

### 与阈值 133,632 B 的对照

**远小于（且为负）**。探针覆盖的四类机制对"消费者迁移后能瘦多少（`S`）"的贡献约等于零
（≤ 15 KB 量级）。盈亏平衡要求每个消费者平均瘦掉 > 133,632 B，因此判据 2 的全部希望
都押在**未覆盖的 `window_host` 与 `pty`** 上——它们合计需要贡献
> 133,632 + 14,848 = **148,480 B** 的静态成本，共享收益才能成立。

### 结论（三档之一）

**「介于两者之间 → 不足以判决，必须做 con 迁移实测」**

- `S_probe`（-14,848 B）**远小于**阈值 133,632 B；
- 但缺口 148,480 B 是否大于 `window_host` + `pty` 的合理体积**没有实测数据**——
  这两块是 `libagenterm.dll`（400,896 B）与 `agenterm-con`（629,760 B）中公认最大的
  两块机制（见 `plan/phase0-baseline-measurements.md`），其静态成本完全可能超过
  148,480 B；
- 本探针**刻意不调用** window/pty（那需要开窗口、起 PTY，见上文），因此无法测得它们，
  这正是 brief 所指的"真实 con 的 `S` 很可能高于 `S_probe`"；
- 唯一能判决的路径是 con 的 dylib 消费变体迁移实测（Phase 0 判据 2 的"迁移后"列）。

**已排除的结论**：档 1（`S_probe` > 阈值）不成立；档 2（"判据 2 很可能不成立"）因缺
window+pty 实测而不能断言——若 window+pty 静态成本 > 148,480 B，判据 2 仍有可能成立。

## 运行输出示例（本机实测）

变体 A：

```
size-probe variant A (static: agenterm-platform rlib)
size-probe[variant A] parent-console write ok
user_config_dir_len=32
default_shell_len=27
clipboard_has_text=true
process_count=371
parent_console_write_stdout=ok
abi_version=0(static placeholder)
```

变体 B：

```
size-probe variant B (dynamic: libagenterm cdylib via libloading)
size-probe[variant B] parent-console write ok
user_config_dir_len=32
default_shell_len=27
clipboard_has_text=true
process_count=370
parent_console_write_stdout=ok
abi_version=65537
```

两边六项数值一致（进程数是瞬态值，采样瞬间可能差一）；只打印长度/布尔/数字，
不打印路径或剪贴板内容。`abi_version=65537` = `(1 << 16) | 1`（ABI 1.1）。
