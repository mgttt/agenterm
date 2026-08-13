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

**静态库与动态库导出同一批 52 个 `agt_*` 符号**（`exports.txt` 为准，
`tests/exports_set.rs` 与 `tests/artifacts.rs` 分别闸住符号集与产物存在性）。

**静态链接时 panic 围栏同样要求 `panic = "unwind"`**：静态库仍必须用
`--profile abi-release` / `abi-dev` 构建，默认 `dev` / `release`（abort）
会被 `compile_error!` 闸挡住。除非开启 `allow-abort-profile`——但那样
构建出的库没有 `catch_unwind` 围栏，只适合没有 C 边界的 Rust 内部消费者。

> **命名**：产物文件名现在是 `libagenterm.{a,so,dylib}` / `agenterm.dll`，
> 与 `plan/plan-v0.1.18.md` §14 一致（里程碑 17 完成改名）。**package 名仍是
> `agenterm-abi`**（Cargo 依赖声明用它）；**lib/crate 名是 `agenterm`**（Rust
> `use agenterm::` 与产物文件名用它）。

## 静态链接（C 消费者，里程碑 18 实测）

静态库文件名：Windows **`agenterm.lib`**，Unix **`libagenterm.a`**，位于
`target/<profile>/`（profile 为 `abi-dev` 或 `abi-release`）。Rust `staticlib`
静态进 C 程序时，**C 侧必须补齐 Rust 运行时的系统库依赖**——这是里程碑 18
实测得出的，不是猜的：先只链静态库，把链接器报的 unresolved symbol 抓下来，
按实际缺失逐个补。

**Windows / MSVC 实测系统库列表**（kernel32 由 MSVC 链接器默认自动链接，
无需显式给出）：

```
cl /nologo /W4 /WX /Iinclude examples/c/agenterm_probe.c target/abi-dev/agenterm.lib ws2_32.lib ntdll.lib ole32.lib user32.lib uxtheme.lib dwmapi.lib /Fe:probe.exe
```

对应符号分布：`ws2_32` = Winsock2（recv/send/accept/WSA*…）、`ntdll` =
NtCreateFile/NtOpenFile/NtWriteFile/RtlGetVersion 等、`ole32` = COM 与
drag-drop、`user32` = 触摸输入、`uxtheme` = SetWindowTheme、`dwmapi` = DWM
窗口属性。以上命令假定已设置 MSVC 工具链环境（`vcvars64.bat`），`cl` 才能
沿 INCLUDE/LIB 找到头文件与系统库。

**gcc / clang（Unix）**——Linux 初始集（若在 CI 上实测报缺符号，按链接器
输出逐个补，并回填这里）：

```
cc -Wall -Wextra -Werror -Iinclude examples/c/agenterm_probe.c target/abi-dev/libagenterm.a -o probe -ldl -lpthread -lm
```

**gcc / clang（macOS）**——两轮 CI 校准所得（里程碑 21b 首轮 + 21c 第二轮）：
macOS 上 `libagenterm.a` 静态进 C 程序时，winit / core-graphics /
core-foundation 会拉入 `_CF*`（如 `_CFAbsoluteTimeGetCurrent`、
`_CFArrayGetCount`、`_CFBundleCopyExecutableURL`）、CG、NS 等符号，
**C 侧必须显式补齐 Apple frameworks**。`-framework X` 是两个独立参数
（先 `-framework` 再写名字），多给 framework 不会导致链接失败，缺了才会；
`-ldl -lpthread -lm` 在 macOS 上无害并保留。第二轮（21c）补的 **Carbon**
是给 winit `platform_impl::macos::event::get_modifierless_char` 的三个
Text Input Services / HIToolbox 符号：`_LMGetKbdType`（HIToolbox 的
Legacy Menu Manager 兼容层）、`_TISCopyCurrentKeyboardLayoutInputSource` /
`_TISGetInputSourceProperty`（Text Input Sources）：

```
cc -Wall -Wextra -Werror -Iinclude examples/c/agenterm_probe.c target/abi-dev/libagenterm.a -o probe -framework CoreFoundation -framework CoreGraphics -framework AppKit -framework Foundation -framework QuartzCore -framework Metal -framework IOKit -framework Carbon -ldl -lpthread -lm
```

> macOS 清单为**两轮 CI 校准所得**（21b 首轮补 `_CF*` / CG / NS 等，21c
> 第二轮补 Carbon 的 3 个键盘符号），本机（Windows）无法验证 macOS 链接；
> 若下一轮 CI 仍报缺符号，按链接器输出逐个补 framework 并回填本表。

**panic 围栏同样适用于静态库**：静态链接**不消除 C 边界**，所以静态库
一样必须用 `--profile abi-release` / `abi-dev` 构建；默认 `dev` / `release`
（abort）会被 `compile_error!` 闸挡住（见上）。`c_static_link.rs` 是这条
路径的链接回归闸：找不到静态库、编译失败、链接失败、运行非 0 都红。

**静态 vs 动态取舍**：静态省去随行 dll（自包含、部署简单），但产物更大、
且升级库要重新链接；动态则运行时加载、升级只需替换库文件。

**给嵌入方的一句话结论（发布形态实测）**：静态链接的自包含产物大约是
**Windows 262 KB / macOS 2.5 MB（strip 后）/ Linux 11.6 MB（strip 后）**；
动态形态探针很小，但**必须随行**对应的 `.dll` / `.so` / `.dylib`——缺少
它们，动态链接的可执行文件无法运行。

**实测体积（探针即 `examples/c/agenterm_probe.c` 链接产物；`abi-dev` /
`abi-release` profile）**：

| 平台 | profile | 动态探针 | 静态探针 | 静态 strip 后 | 静态库归档 |
|------|---------|----------|----------|---------------|------------|
| Windows (x86_64 + MSVC) | abi-dev | 142,848 B（CI）/ 139,776 B（本机） | 311,808 B（CI）/ 310,784 B（本机） | 不适用（调试信息在 `.pdb`） | 31,581,304 B（本机） |
| | abi-release | 139,776 B（本机） | 261,632 B（本机） | 不适用（调试信息在 `.pdb`） | 19,893,678 B（本机） |
| macOS | abi-dev | 34,160 B | 17,431,072 B | 8,855,608 B | 57,723,624 B |
| | abi-release | 34,160 B | 4,857,872 B | 2,543,240 B | 33,141,424 B |
| Linux | abi-dev | 16,480 B | 133,516,320 B | 35,256,016 B | 281,754,874 B |
| | abi-release | 16,480 B | 26,791,488 B | 11,601,544 B | 133,007,220 B |

> ⚠️ **口径说明（必读）**：
> - **`abi-dev` 与 `abi-release` 不可混用**：dev（未优化 + 带调试信息）与
>   release（优化）产物差异巨大，按目标形态选 profile，不要拿 dev 数字当
>   发布形态预算；
> - **Windows 与 Unix 不可直接比较**：MSVC 把调试信息放在独立 `.pdb`，
>   Unix 把 DWARF 直接嵌进二进制——静态列 dev 下看似差 400 倍
>   （312 KB vs 17/133 MB）正是调试信息存放方式不同所致；
> - **strip 只对 Unix 适用**：Unix 行在 `abi-*` 构建后执行 `strip`（静态
>   strip 后列）；Windows 无 strip 行，调试信息本来就不在二进制里。

> **"单消费者静态更省"只在 Windows 成立**：里程碑 25 曾断言"单消费者静态
> 链接总字节更小（310,784 vs 540,672）"。按**发布形态**数字重新表述：
> Windows 上静态 261,632 B vs 动态探针 139,776 B + `agenterm.dll`
> 400,896 B = **540,672 B**，静态更省；**Linux/macOS 上静态（strip 后
> 11.6 MB / 2.5 MB）远大于动态探针 + 库**，那条结论在 Unix 不成立。

数字来自 `c_static_link.rs` / `c_consumer.rs` 的打印（链接后 `fs::metadata`
实测，无体积断言）。Linux/macOS 为 **CI 实测**（CI run 31681964185，
三平台全绿；gcc / clang 链，DWARF 嵌入二进制），Windows 为本机实测（MSVC；
CI 与本机并列处均已标注，两者差异属 MSVC 版本不同，如实并列）。动态形态
除上表探针外**仍需随行** dll/so/dylib（本机 Windows `abi-dev` 实测
`agenterm.dll` = 878,080 B；`abi-release` 基线 400,896 B 见
`plan/phase0-baseline-measurements.md`）。

**静态库的符号面远大于动态库（里程碑 36 实测，里程碑 39 三平台实测）**：
选静态还是动态，除了体积（上表）还应看符号面——这是**只有静态链接才有的
风险**，不试看不出来。修好测量仪器后的三平台实测（CI run 31692909368，
三平台全绿；数字照实测，未重算）：

| 平台 | 动态库导出 | 其中 `agt_*` | 静态归档外部符号 | 其中 `agt_*` | Rust mangled |
|------|-----------|-------------|-----------------|-------------|--------------|
| Windows (`dumpbin`) | **41** | 41 | **7,247** | 41 | 5,329 |
| Linux (`nm -D`) | **41** | 41 | **44,377** | 41 | 43,964 |
| macOS (`nm -gU`) | **45** | 41 | **5,897** | nm -gU 未列出（见注） | 5,568 |

**结论一句话**：动态库在 Windows/Linux 上导出的**就是那 41 个 ABI 符号**，
macOS 多 4 个 ObjC 类注册符号（结构性必需）；**静态归档三平台都暴露上千到
四万多个符号**。

macOS 动态库多出的 4 个符号（已查明，不是泄漏）：

```
___CLASS_SoftbufferObserver
___DROP_FLAG_OFFSET_SoftbufferObserver
___IVAR_OFFSET_SoftbufferObserver
___REGISTER_CLASS_SoftbufferObserver
```

这是 **`softbuffer` crate 在 macOS 上注册 Objective-C 类**所需的运行时符号
（ObjC 运行时要靠它们注册类），**属于结构性必需，不是泄漏**。

> **macOS 归档 `agt_*` 符号：`nm -gU` 未列出（已查明，非产物缺陷）**：
> `nm -gU` 对 macOS 归档**不列出** `agt_*` 符号——诊断采样为空
> （`agt sample: <空>`），排除了 grep 模式与取字段两种解释；但**符号确实存在**：
> `c_static_link` 闸在 macOS 上静态链接 `libagenterm.a` 并解析全部 41 个
> `agt_*`，**通过**——链接成功即符号存在的证明。因此这是**工具列举行为的差异，
> 不是产物缺陷**；`total=5,897` 与 `rust=5,568` 两个数仍然有效（它们确实被
> 列出来了）。

动态库只暴露 41 个 ABI 符号（macOS 45 个，多出的 4 个是上述结构性 ObjC 类
注册符号）；静态库把整个 Rust std/core/platform 符号面暴露给链接它的人。
后果与缓解：

- 消费者若**同时链接另一个 Rust staticlib**（另一个 Rust 写的库），两个归档
  会各带一份 std/core 符号 → **重复符号冲突**；
- **Linux 归档 44,377 个符号是 Windows 的 6 倍**，在 Linux 上与其它 Rust
  静态库共存的风险相应更高；
- Rust 的 mangled 名带 crate 哈希（如 `Cs4ADFETM7JMv_8agenterm`）能降低但
  **不能消除**冲突——同一 rustc 版本编出的 std 符号是一样的；
- 动态库完全没有这个问题（Windows/Linux 只导出 41 个）。

**建议（陈述事实与选项）**：

- 只链接**一个** Rust staticlib 时通常没问题——本仓的 C 闸
  （`c_static_link.rs`）就是这种情形，实测通过；
- 需要与其它 Rust 库共存时，**动态库是更安全的形态**。

> **测量口径与来源**：Windows 本机 `dumpbin`（`/EXPORTS` 与 `/SYMBOLS`），
> Linux `nm -D`，macOS `nm -gU`；Linux/macOS 为 CI run 31692909368 实测
> （三平台全绿），Windows 为本机实测。

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

当前为 `0x00010003`（major=1, minor=3）：里程碑 2–10 陆续新增了
PTY / window / frame / input / screenshot / process / clipboard /
parent-console / runtime / a11y 等大量向后兼容导出，minor 随导出面增长
（含 `agt_a11y_node_set_text` / `agt_a11y_node_send_keys`）。

`agt_build_id()` 返回 `<crate 版本>+abi.<major>.<minor>`
（例如 `0.1.16+abi.1.1`），在**编译期**由 `CARGO_PKG_VERSION` 与
`ABI_MAJOR` / `ABI_MINOR` 常量拼接而成——不手写字面量，crate 版本或 ABI
常量一改，build id 自动跟随，不会过期。字符串以 NUL 结尾、静态、永久有效。

## 测试

- `tests/exports_set.rs`：导出符号集与 `exports.txt` 完全一致（编译期不改 ABI）。
- `tests/dylib_load.rs`：用 `libloading` 加载真实 cdylib，调用导出并断言
  返回的 `const char*` 均为合法 NUL 结尾 C 字符串（缺陷回归闸）。找不到
  cdylib 时该测试直接失败（先执行上面的 build 命令）。

## 平台契约：macOS 窗口宿主（里程碑 22 定为契约）

窗口循环线程模型（库内私有线程跑 `run_pixel_window`）目前只在 **Windows**
验证（消息泵归创建线程）。**macOS 是硬契约而非待办限制**：AppKit 要求窗口/
事件循环在主线程，而本 ABI 把它放在库内私有线程，因此在 macOS 上：

- `agt_window_open` **恒返回 `AGT_UNSUPPORTED`**（`code="unsupported_platform"`，
  message 说明 "macOS requires the event loop on the main thread; this ABI
  hosts it on a library-private thread"），不创建私有循环线程、不调用
  `run_pixel_window`（winit 会 panic 且污染全局状态，重试永远不可能成功，
  所以这不是 `AGT_FAILED`）；
- `agt_capability_query(AGT_CAP_WINDOW_HOST)` 同步返回 `AGT_UNSUPPORTED`，
  与 `agt_window_open` 行为一致（Windows/Linux 维持 `AGT_OK`）。

主线程宿主留待后续里程碑。`include/agenterm.h` 亦写明此契约（纯 ASCII）。

`agenterm-cu` 的 `tree` / 结构化 `click` / `focus` 在 Linux `current` 上经
本 crate 的 `agt_a11y_*` 机制消费；`windows` / `screenshot` / 坐标降级输入仍
直连 `agenterm-platform`，待对应 ABI 里程碑落地后迁入。
