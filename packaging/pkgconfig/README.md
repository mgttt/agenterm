# libagenterm pkg-config 消费元数据（里程碑 42）

`libagenterm.pc.in` 是 pkg-config 模板；替换占位符后得到 `libagenterm.pc`，
C 构建系统靠它知道三件事：头文件目录（`Cflags`）、链哪个库（`Libs`）、
**静态链接时还要补哪些系统库**（`Libs.private`）。最后一条来自实测
（里程碑 18 / 21b / 21c），此前只写在 `crates/agenterm-abi/README.md`
的散文里，喂不进构建系统——pkg-config 的 `Libs.private` 就是放
"静态链接时还需要什么"的地方。

## 三平台 `Libs.private` 实测值

> **下表是防漂移闸的解析锚点**：`crates/agenterm-abi/tests/pkgconfig_libs.rs`
> 逐字比对表格里的清单与 `crates/agenterm-abi/tests/c_static_link.rs`
> 实际链接用的清单（代码侧唯一真相源是
> `crates/agenterm-abi/tests/common/mod.rs` 的 `system_libs` 常量）。
> 改清单请先改那里的常量，再同步本表；请保持三行 `| <平台> |` 表行
> 及其反引号单元格的格式不变。

| 平台 | `Libs.private` 值（实测） |
|------|--------------------------|
| Windows/MSVC | `ws2_32 ntdll ole32 user32 uxtheme dwmapi` |
| Linux | `-ldl -lpthread -lm` |
| macOS | `-framework CoreFoundation -framework CoreGraphics -framework AppKit -framework Foundation -framework QuartzCore -framework Metal -framework IOKit -framework Carbon -ldl -lpthread -lm` |

来源（全部实测，不是猜的）：

- **Windows/MSVC**：里程碑 18 实测——先只链静态库，把 MSVC 链接器报的
  unresolved symbol 逐个补成这六个库；`kernel32` 由 MSVC 链接器默认自动
  链接，无需显式给出。符号分布：`ws2_32` = Winsock2、`ntdll` = Nt* /
  RtlGetVersion、`ole32` = COM 与 drag-drop、`user32` = 触摸输入、
  `uxtheme` = SetWindowTheme、`dwmapi` = DWM。
- **Linux**：里程碑 18 链接闸在 CI 上实测。
- **macOS**：里程碑 21b（首轮 `_CF*` / CG / NS 符号，来自 winit /
  core-*）+ 21c（第二轮补 Carbon 的 3 个 winit
  `get_modifierless_char` Text Input Services / HIToolbox 符号）两轮 CI
  校准所得。`-framework X` 是两个独立参数（先 `-framework` 再写名字，
  不是 `-framework=X`）；`-ldl -lpthread -lm` 在 macOS 上无害并保留。
  仍是 CI 校准集：若下一轮 CI 报缺符号，按链接器输出补 framework 后
  同步改 `tests/common/mod.rs` 与本表。

## Windows 用户：pkg-config 不是主流，走命令行

Windows/MSVC 上 pkg-config 并非主流。**MSVC 用户请直接看
`crates/agenterm-abi/README.md` 的「静态链接（C 消费者，里程碑 18 实测）」
一节**——那里有完整可复制的 `cl` 命令行，含带 `.lib` 后缀的链接参数
（`ws2_32.lib ntdll.lib ole32.lib user32.lib uxtheme.lib dwmapi.lib`），
不经过 pkg-config。本模板的 `Libs.private` 对 Windows 记裸名
（`ws2_32 ntdll ...`）以便用 `-l` 形式传给 MSVC 兼容链接器，但这里如实
说明：**不要假装 Windows 也走 pkg-config**，`.pc` 主要服务 Unix 构建
系统（如 `pkg-config --libs --static libagenterm`）。

## 如何填模板（谁替换占位符）

占位符 `@PREFIX@` / `@LIBDIR@` / `@INCLUDEDIR@` / `@VERSION@` /
`@SYSTEM_LIBS@` 由**打包脚本**在安装时替换（把占位符换成实际安装路径与
版本），也可以手工替换。替换后的 `libagenterm.pc` 应安装到 pkg-config
的搜索路径（`pkg-config --variable pc_path pkg-config` 可查）。各占位符：

- `@PREFIX@`：安装前缀，如 `/usr/local`；
- `@LIBDIR@`：存放 `libagenterm.{a,so,dylib}` 的目录；
- `@INCLUDEDIR@`：存放 `agenterm.h` 的目录；
- `@VERSION@`：`crates/agenterm-abi` 的 crate 版本（见其 `Cargo.toml`）；
- `@SYSTEM_LIBS@`：按目标平台取上表对应值（Windows 取裸名行，Linux /
  macOS 取参数行）。

## 防漂移闸

`crates/agenterm-abi/tests/pkgconfig_libs.rs` 断言上表记录的三平台清单与
`c_static_link.rs` **实际链接用的清单**逐字一致，并检查 `.pc.in` 模板的
关键结构（`Name` / `Description` / `Version` / `Cflags` / `Libs` /
`Libs.private` 与 `-lagenterm`）。将来谁改了链接参数（例如新依赖引入新
framework）而不同步本表，测试当场红并指出差在哪——消费者拿过期清单
链接失败的问题由此提前暴露在 CI 里。
