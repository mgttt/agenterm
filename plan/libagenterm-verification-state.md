# libagenterm 交付面验证状态（2026-08-14 收口）

> 本文档回答一个问题：**libagenterm 对外承诺的每一条，今天有没有一个"错了会红"的检查盯着？**
> 它不是设计文档（那是 `plan-v0.1.18.md` §14），也不是 Phase 0 判据的结论
> （那是 `phase0-baseline-measurements.md`）。它是**验证覆盖面的现状台账**，
> 以及**还压在人身上的决策清单**。

## 1. 已被门控的承诺

每一行的"门"都跑在三个平台的 `ci-libagenterm.yml` 上（除非注明）。

| 承诺 | 门 | 关键点 |
|---|---|---|
| 导出集恰好是 `exports.txt` 所列 | `exports_set.rs` | 源码三方：清单 ↔ 头文件 ↔ `#[no_mangle]` |
| 导出名是机制不是产品 | `exports_set.rs` | 按下划线分段匹配，不是子串（`parent_console` 含 "con" 不得误报） |
| 55 个符号真在产物里 | `symbol_presence.rs` | 动态逐个 `get()`；静态**从清单生成取地址表并链接** |
| 动态导出面**恰好**是承诺的那些 | `export_exactness.rs` | 用 `object` 解析 ELF/Mach-O/PE，双向断言。macOS 允许清单含 4 个 objc2 泄漏符号（见 §3） |
| 归档里每个全局符号都有归类与安全理由 | `archive_surface.rs` | 无兜底类别；libm 裸名**仅在弱绑定时**准入 |
| C 消费者能编译、链接、运行 | `c_consumer.rs` / `c_static_link.rs` | 动态与静态各一 |
| **C++** 消费者同上 | `cpp_consumer.rs` | 反证过：停用 `extern "C"` 守卫则链接期出现修饰名 |
| 窗口/帧会合可用 | `c_window.rs` / `c_window_static.rs` | **静态归档也能驱动完整会合**，与动态版输出逐行一致 |
| 空指针/非法句柄一律干净失败 | `null_sweep.rs` | 覆盖面本身被门控：47 扫 + 8 豁免 = 55，豁免签名含指针即红 |
| 能力枚举编号三处一致 | `capability_enum_gate.rs` | 比名字**和**数值、按声明顺序（只比名字集合会漏掉对调） |
| pkg-config 元数据可用 | `pkgconfig_libs.rs` / `pkgconfig_consume.rs` | 四方防漂移（见 §2）+ 端到端真链真跑 |
| 安装布局正确 | `install_consume.rs` | 跑真实 `install.sh`，**运行时只指向安装目录，不许回落 `target/`** |
| 共享库有安装身份 | `install_identity.rs` | Linux `DT_SONAME`、macOS `@rpath` install name |
| 消费者拒绝 ABI major 不匹配 | `agenterm-cu` 单测 + 真实产物门控 | 校验在 `load()` 一处，不匹配的库根本没机会被调用 |
| 一个进程只选一种链接形态 | `mixed_linkage.rs` | 三平台实测两份错误状态独立；规则写进头文件 |
| 开着窗口时枚举不死锁 | `enumerate_while_hosting.rs` | 见 §3 的修复 |
| `agt_native_window_*` 成功路径 | `native_window_ops.rs` | 对**子进程**开的普通窗口操作（不是 ABI 托管窗口，头文件禁止那样用）。每次调用前按标题 + `process_id` 重核归属；`close` 后断言子进程退出码 0 |

## 2. 系统库清单：四方一致

`ws2_32 ntdll ole32 user32 uxtheme dwmapi`（Windows）等清单存在四份拷贝，
`pkgconfig_libs.rs` 断言四者一致：

1. `packaging/pkgconfig/README.md` 表格
2. `crates/agenterm-abi/tests/common/mod.rs::system_libs`（代码侧真相源）
3. `packaging/pkgconfig/generate-pc.sh` 内嵌值
4. `crates/agenterm-abi/README.md` 的 `cl` 命令行 ← **Windows 用户实际复制的那份**

第 4 份长期无人看管，而 pkg-config README 恰恰把 MSVC 用户指向它。

## 3. 修掉的真缺陷（都属于"没人看所以没人知道"）

| 缺陷 | 表现 | 根因 |
|---|---|---|
| 枚举死锁 | 持有 ABI 窗口时 `agt_window_enumerate` 永不返回，进程不退、窗口留在桌面 | `GetWindowTextW` 对**本进程**窗口发 `WM_GETTEXT`，而循环线程停在帧会合点。改用 `SendMessageTimeoutW` + 100ms 上界 |
| macOS 共享内存全废 | `shm_open` 返回 EINVAL | 传了 `O_CLOEXEC`；macOS 不接受该标志。改为事后 `fcntl(F_SETFD)`。platform 单测**只在 ubuntu 跑**，故从未在 macOS 执行过 |
| `CGDisplayBounds` 双声明 | clippy `clashing_extern_declarations` | 两模块各定义一个 `CgRect`，布局相同、名义不同。**不是运行时 bug**，但改动任一形状会让另一处静默读错偏移 |

**未修、已记录**：macOS dylib 多导出 4 个 objc2 类注册符号。两条链接器路线都被实测堵死——
rustc 必然自带 `-exported_symbols_list` 且 ld64 取并集；`-unexported_symbols_list`
被 ld64 拒绝（两种形式不能混用）。故保留为**带理由的允许清单**，第 5 个泄漏仍会红。

## 4. 压在人身上的决策

以下三件**不是工程问题**，我不替你定：

1. **判据 2 的处置。** 探针级负面证据已有（结论档位「很可能不成立」）。
   §14.6 的规则是"判据不过 → §14 整节删除，§9 留一行否决理由与数字"。删不删是你的决定。
2. **判据 3 的 journey 或阈值。** 实测：同一二进制自比波动 33%–50%，比 5% 阈值高一个量级；
   **且放慢节奏无效**（每步 +80ms 后帧数不变，因为 `pty_drained_bytes` 恒为 0——
   resize 一个空闲终端本就不产生渲染工作）。要提高样本量必须让它真渲染或换指标。
   在此之前，**建出 con 的 dylib 变体也判不出结果**。
3. **要不要投 con 的 dylib 变体。** 直接受第 2 条影响：现在投下去换不来判据 3 的结论。
4. **`agt_input_*` 的成功路径要不要测、怎么测。**
   `agt_input_pointer_move` / `pointer_click` / `type_text` / `send_keys`
   目前**只有失败路径覆盖**（`null_sweep.rs`；`pointer_move` 连空扫都无从下手，
   它不收指针，已在豁免清单里注明）。成功路径零证据，而
   `agenterm-cu` 的 `executor.rs` 有 7 处在用它们。

   难点不是技术，是**输入注入是全局的**：即便目标是我们自己子进程的窗口，
   `SendInput` 也会移动真实光标、把按键送进当前焦点窗口。本仓一贯的安全约束
   是"测试绝不注入鼠标点击或按键"，在有人正在使用的桌面上这条应当保持。

   可行方案（未采纳，待定）：测试第一步检查
   `AGENTERM_ALLOW_INPUT_INJECTION=1`，未设置即 `SKIP:`，只有 CI 的 workflow
   设它；注入目标是子进程自己的窗口，由子进程回报收到的
   `WM_CHAR` / `WM_LBUTTONDOWN`。**本轮未实施** —— 是否引入任何会注入输入的
   代码，由人决定。

## 5. 范围外、已测量、未动

`agenterm-rh`（5 个）、`agenterm-qjs`（1 个）、`agenterm-wasmcore`（4 个）
共 **10 个集成测试文件没有任何 workflow 跑**。它们属于脚本引擎路线，
不属于 libagenterm，故只报数字不动手。
