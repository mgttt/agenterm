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
> `crates/agenterm-abi/tests/common/mod.rs` 的 `system_libs` 常量），并与
> `packaging/pkgconfig/generate-pc.sh` 内嵌的 `SYSTEM_LIBS_LINUX` /
> `SYSTEM_LIBS_DARWIN` 比对（三方一致）。
> 改清单请先改那里的常量，再同步本表与脚本内嵌值；请保持三行
> `| <平台> |` 表行及其反引号单元格的格式不变。

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

## 生成 .pc：packaging/pkgconfig/generate-pc.sh（里程碑 52）

`packaging/pkgconfig/generate-pc.sh` 是 `.pc.in` 的正式生成脚本（POSIX
`sh`；`.pc` 是 Unix 交付通道，不提供 Windows 版）。在仓库根目录运行：

```sh
sh packaging/pkgconfig/generate-pc.sh \
    --prefix /usr/local \
    --version 0.1.16 \
    --output /usr/local/lib/pkgconfig/libagenterm.pc
```

- `--libdir` 默认 `$prefix/lib`，`--includedir` 默认 `$prefix/include`；
- `--version` 取 `crates/agenterm-abi` 的 crate 版本（见其 `Cargo.toml`）；
- `@SYSTEM_LIBS@` 按 `uname -s` 自动选择（内嵌 Linux / Darwin 两组值，
  与下表及 `tests/common/mod.rs::system_libs` 三方一致，见防漂移闸）。
  **不认识的平台（含 Windows 上的 MSYS/MINGW）明确报错退出，绝不输出
  空的 `Libs.private`**；
- 生成后脚本自检一遍：任何非注释行残留 `@...@` 占位符都会非零退出并
  报出是哪个占位符；
- `--system=linux` / `--system=darwin` 强制指定平台组，**仅供测试与交叉
  打包**使用（例如在 macOS 上为 Linux 目标生成，或 CI 在 Windows 上验证
  脚本逻辑）；常规安装保持默认 `auto`。

真实输出（本仓库在 Windows/Git Bash 上以 `--system=linux` 验证——`auto`
在 MSYS 上按设计报错，见下）：

```sh
$ sh packaging/pkgconfig/generate-pc.sh --prefix /usr/local --version 0.1.16 --system linux --output /tmp/m52-readme/libagenterm.pc
generate-pc.sh: wrote /tmp/m52-readme/libagenterm.pc (Libs.private selected for: linux)
$ cat /tmp/m52-readme/libagenterm.pc
# libagenterm pkg-config template (milestone 42).
#
# Placeholders are substituted by the packaging script at install time (or
# by hand — see packaging/pkgconfig/README.md):
#   @PREFIX@      install prefix (e.g. /usr/local or a ~/.local prefix)
#   @LIBDIR@      directory containing libagenterm.{a,so,dylib}
#   @INCLUDEDIR@  directory containing agenterm.h
#   @VERSION@     library version (the agenterm-abi crate version)
#   @SYSTEM_LIBS@ per-platform system libraries a STATIC consumer must also
#                 link; the measured per-platform values and their sources
#                 are recorded in packaging/pkgconfig/README.md. The
#                 anti-drift gate crates/agenterm-abi/tests/pkgconfig_libs.rs
#                 asserts those values stay in sync with the lists the
#                 c_static_link.rs link test actually uses.
#
# Standard pkg-config variables so ${prefix}/... composition works for
# consumers that read libdir/includedir.
prefix=/usr/local
exec_prefix=${prefix}
libdir=/usr/local/lib
includedir=/usr/local/include

Name: libagenterm
Description: agenterm mechanism ABI (C boundary to OS terminal/window/process mechanisms)
Version: 0.1.16
Cflags: -I${includedir}
Libs: -L${libdir} -lagenterm
Libs.private: -ldl -lpthread -lm
```

生成的 `.pc` 头部注释保留模板的占位符说明：脚本只替换非注释行
（`/^#/!`），注释原样保留，避免把占位符说明文字替换成实际路径。
在 MSYS/Windows 上不指定 `--system` 的行为（明确报错，绝不输出空
`Libs.private`）：

```sh
$ sh packaging/pkgconfig/generate-pc.sh --prefix /tmp/x --version 0.1.16 --output /tmp/x.pc
generate-pc.sh: unsupported platform 'MINGW64_NT-10.0-20348' -- refusing to emit an empty Libs.private. This script serves Unix; use --system=linux|darwin only for tests and cross packaging.
$ echo $?
1
```

## 如何填模板（谁替换占位符）

占位符 `@PREFIX@` / `@LIBDIR@` / `@INCLUDEDIR@` / `@VERSION@` /
`@SYSTEM_LIBS@` 由 `generate-pc.sh`（上节）在安装时替换，也可以手工替换。
替换后的 `libagenterm.pc` 应安装到 pkg-config 的搜索路径
（`pkg-config --variable pc_path pkg-config` 可查）。各占位符：

- `@PREFIX@`：安装前缀，如 `/usr/local`；
- `@LIBDIR@`：存放 `libagenterm.{a,so,dylib}` 的目录；
- `@INCLUDEDIR@`：存放 `agenterm.h` 的目录；
- `@VERSION@`：`crates/agenterm-abi` 的 crate 版本（见其 `Cargo.toml`）；
- `@SYSTEM_LIBS@`：按目标平台取上表对应值（Windows 取裸名行，Linux /
  macOS 取参数行）。

## 防漂移闸

`crates/agenterm-abi/tests/pkgconfig_libs.rs` 断言**三方一致**：

1. 上表记录的三平台清单；
2. `c_static_link.rs` 实际链接用的清单（代码侧唯一真相源是
   `crates/agenterm-abi/tests/common/mod.rs` 的 `system_libs` 常量）；
3. `generate-pc.sh` 内嵌的 `SYSTEM_LIBS_LINUX` / `SYSTEM_LIBS_DARWIN`
   两组值——脚本真正写给消费者的 `Libs.private`。

任一漂移当场红并指出是哪一方对不上（README 表 / 常量 / 脚本）。将来谁
改了链接参数（例如新依赖引入新 framework）而不同步三者，测试当场红并
指出差在哪——消费者拿过期清单链接失败的问题由此提前暴露在 CI 里。

**端到端**由 `crates/agenterm-abi/tests/pkgconfig_consume.rs` 兜底（里程碑
52）：它把 `libagenterm.a` 与 `agenterm.h` 拷进临时 staging 树，用仓库里
的 `generate-pc.sh` 生成 `libagenterm.pc`（被测的必须是发布物，不许在测试
里再写一份替换逻辑），再以 `PKG_CONFIG_PATH` 跑真实的
`pkg-config --cflags --libs --static libagenterm`，把链接行原样喂给真实
C 工具链编译链接并运行，断言退出码 0、输出含 `-lagenterm`、无残留 `@`。
仅 Unix 上真跑；Windows 与无 `pkg-config` 可执行文件时按统一格式显式
`SKIP:`（`.pc` 是 Unix 交付通道）。消费者侧样例见下节。

## 消费者侧：pkg-config 用法

把生成的 `libagenterm.pc` 装进 pkg-config 搜索路径后（
`pkg-config --variable pc_path pkg-config` 可查，或把所在目录加进
`PKG_CONFIG_PATH`），C 构建系统直接查：

```sh
cc $(pkg-config --cflags --libs libagenterm) agenterm_probe.c -o probe            # 动态
cc $(pkg-config --cflags --libs --static libagenterm) agenterm_probe.c -o probe   # 静态
```

`--static` 会把 `Libs.private`（静态链接必须补的系统库）追加进链接行。

对应上面 `--prefix /usr/local` 生成的 `.pc`（Linux），
`pkg-config --cflags --libs --static libagenterm` 的输出为：

```text
-I/usr/local/include -L/usr/local/lib -lagenterm -ldl -lpthread -lm
```

macOS 上 `Libs.private` 以 `-framework` **参数对**展开（`-framework` 与
名字是两个独立参数，`pkg-config --static` 会原样吐出，例如
`-framework CoreFoundation -framework CoreGraphics ... -ldl -lpthread
-lm`）。注意：本 README 编辑于 Windows 主机（无 pkg-config 可执行文件），
上面的输出是对生成的 `.pc` 的确定性展开；真实 pkg-config 输出由 CI 的
`pkgconfig_consume` 端到端测试在 Linux/macOS 上逐字验证，若与上表
`Libs.private` 实测值有任何出入，测试当场红。

## 一个进程只选一种链接形态（里程碑 59）

本仓同时交付动态库与静态库,**打包时不要让同一个进程两者都拿到**。
典型出事路径:主程序静态链 `libagenterm.a`,又加载一个 dlopen
`libagenterm.so` 的插件 —— 进程里就有两份副本,**错误状态互相独立**。

这是 `tests/mixed_linkage.rs` 在 Windows / Linux / macOS 上**实测**的,
不是推断:通过一份副本触发失败,另一份的 `agt_last_error` 读不到;
更糟的是它会继续报**自己上一次**记下的错误,于是调用方读到一条
看起来合理、其实属于另一次调用的诊断。`agt_a11y_tree_snapshot` 与其
访问器之间的快照、以及各类句柄(`agt_pty_t` / `agt_window_t`)同理 ——
谁创建的就由谁读。

规则写在 `include/agenterm.h` 的 `agt_last_error` 声明旁边,
上面那条测试把这四条实测行为钉住了:哪天隔离性变了(往任一方向),
测试会红,而不是让头文件里的说法悄悄变成假话。

## 安装身份：`DT_SONAME` 与 `@rpath`（里程碑 56）

库被安装（`libagenterm.so` / `libagenterm.dylib` 落到 `$libdir`）之后，
消费者在**运行时**靠什么找到它，由库自己的"安装身份"决定——
`crates/agenterm-abi/build.rs`（里程碑 56）设置，`tests/install_identity.rs`
实测并门控：

- **Linux**：构建时写入 ELF `DT_SONAME = libagenterm.so.1`（`1` 是 ABI
  **主**版本，见 `src/lib.rs` 的 `abi_version!(1, 6)`，不跟 crate 版本
  0.1.16）。消费者链接时记下这个 SONAME，安装后动态加载器按它定位；
  soname 不变时升级补丁版本无需重链接。
- **macOS**：构建时写入 `LC_ID_DYLIB` 的 install name
  `@rpath/libagenterm.dylib`（而不是 cargo 默认的构建树绝对路径）。
  消费者记录 `@rpath/libagenterm.dylib`，**运行时由消费者自己的 rpath
  设置**（例如 `-Wl,-rpath,@loader_path` 或指向 `$libdir` 的 rpath）解析。

`.pc` 的 `Libs` 目前**不带** `-Wl,-rpath`：静态消费者不受影响（rpath 只
对动态加载有意义），动态消费者需要自己给出 rpath 或在 `$libdir` 安装
（`ldconfig` / dyld 的默认搜索路径可覆盖）。若 CI 证明某条动态消费链因此
缺库，先把证据贴出来再讨论是否给 `.pc` 补 rpath——那是对外合同
（`libagenterm.pc.in`），不经证据不改。

## 真安装：`packaging/install.sh`（里程碑 61）

`generate-pc.sh` 只生成 `.pc`，从来没把构建产物真正"装"进一个 prefix；
而里程碑 56 证明安装布局不平凡——Linux 的 `DT_SONAME` 意味着消费者
`DT_NEEDED` 记的是 `libagenterm.so.1`，磁盘上必须有**那个版本化文件**，
`libagenterm.so` 只是链接期符号链接；cargo 又只产出裸名
`libagenterm.so`。`packaging/install.sh` 就是补上的真实安装步骤：它把
构建产物按标准布局装到 prefix，装完自检，并且是幂等的（同一 prefix 装
两次结果一致）。**端到端硬证据**是
`crates/agenterm-abi/tests/install_consume.rs`（里程碑 61）：它建临时
prefix、跑真正的 `install.sh`、只用安装出来的树做 pkg-config 动态与静态
消费并运行 probe——本机是 Windows 只能 `SKIP:`，真实运行发生在 CI 的
linux / macos job（`AGENTERM_ABI_PROFILE_DIR=target/abi-release`）。

一条真实可复制的命令行（从仓库根、Windows/Git Bash 以 `--system linux`
验证——`auto` 在 MSYS 上按设计报错，见下）：

```sh
$ sh packaging/install.sh --prefix /tmp/agenterm-m61-prefix/installed \
    --artifacts /tmp/agenterm-m61-prefix/artifacts --system linux
generate-pc.sh: wrote /tmp/agenterm-m61-prefix/installed/lib/pkgconfig/libagenterm.pc (Libs.private selected for: linux)
install.sh: installed libagenterm 0.1.16 (linux layout) into /tmp/agenterm-m61-prefix/installed
install.sh:   include: /tmp/agenterm-m61-prefix/installed/include/agenterm.h
install.sh:   lib:     /tmp/agenterm-m61-prefix/installed/lib/libagenterm.a
install.sh:   lib:     /tmp/agenterm-m61-prefix/installed/lib/libagenterm.so.1
install.sh:   lib:     /tmp/agenterm-m61-prefix/installed/lib/libagenterm.so -> libagenterm.so.1
install.sh:   pc:      /tmp/agenterm-m61-prefix/installed/lib/pkgconfig/libagenterm.pc
$ find /tmp/agenterm-m61-prefix/installed \( -type f -o -type l \) | sort
/tmp/agenterm-m61-prefix/installed/include/agenterm.h
/tmp/agenterm-m61-prefix/installed/lib/libagenterm.a
/tmp/agenterm-m61-prefix/installed/lib/libagenterm.so
/tmp/agenterm-m61-prefix/installed/lib/libagenterm.so.1
/tmp/agenterm-m61-prefix/installed/lib/pkgconfig/libagenterm.pc
```

参数（与 `generate-pc.sh` 同风格）：

- `--prefix <dir>`（必需）、`--libdir <dir>`（默认 `$prefix/lib`）、
  `--includedir <dir>`（默认 `$prefix/include`）；
- `--artifacts <dir>`：产物目录，默认从仓库推断 `target/abi-release`
  （CI 的 linux / macos job 用 `--profile abi-release` 构建，产物正好在
  那里）；本机开发若用 `abi-dev` 构建，显式指过去即可；
- `--version <ver>`：写进 `.pc` 的版本，默认读
  `crates/agenterm-abi/Cargo.toml`（不传就是在验证这条默认路径）；
- `--system auto|linux|darwin`：默认 `auto` 按 `uname -s` 选，**其他
  平台（含 Windows 的 MSYS/MINGW）明确报错退出**，绝不装出半个布局；
  `linux` / `darwin` 显式覆盖仅供测试与交叉打包：

  ```sh
  $ sh packaging/install.sh --prefix /tmp/agenterm-m61-prefix/x --artifacts /tmp/agenterm-m61-prefix/artifacts
  install.sh: unsupported platform 'MINGW64_NT-10.0-20348' -- refusing to install half a layout. This script serves Unix; use --system=linux|darwin only for tests and cross packaging.
  $ echo $?
  1
  ```

装出来的布局（Linux 与 macOS 各一条，均本机真实跑出）：

| 平台 | 布局 |
|------|------|
| Linux 布局 | `$includedir/agenterm.h`；`$libdir/libagenterm.a`；`$libdir/libagenterm.so.1`（真身）+ `$libdir/libagenterm.so -> libagenterm.so.1`（链接期符号链接）；`$libdir/pkgconfig/libagenterm.pc` |
| macOS 布局 | `$includedir/agenterm.h`；`$libdir/libagenterm.a`；`$libdir/libagenterm.dylib`（install name 已是 `@rpath/libagenterm.dylib`，**不需要**版本化文件名）；`$libdir/pkgconfig/libagenterm.pc` |

**`.1` 从哪来、为什么不会与 `build.rs` 漂移**：`install.sh` 从
`crates/agenterm-abi/src/lib.rs` 的 `abi_version!(<major>, ...)` 解析 ABI
主版本（`sed` 提取第一个 `abi_version!(<数字>,` 的数字，校验必须是数字），
与 `build.rs` 的漂移守卫**同源**——`build.rs` 断言 `src/lib.rs` 含
`abi_version!(1,`，ABI major 一改它自己编译失败；`install.sh` 从不硬编码
`.1`，同一行源解析到新 major 自动跟上。两者派生自同一个源头，不存在第
二处可遗忘。不用 `<OUT_DIR>/soname.txt` 是因为安装脚本不在 Cargo 构建
里运行、拿不到 `OUT_DIR`。

**消费者怎么用**（装完之后）：

```sh
# 动态消费：链接行来自 pkg-config（不带 --static）
PKG_CONFIG_PATH=/tmp/agenterm-m61-prefix/installed/lib/pkgconfig \
  cc $(pkg-config --cflags --libs libagenterm) examples/c/agenterm_probe.c -o probe
# 运行时只靠安装目录被找到——绝不把 target/ 塞进搜索路径
LD_LIBRARY_PATH=/tmp/agenterm-m61-prefix/installed/lib ./probe   # Linux
DYLD_LIBRARY_PATH=/tmp/agenterm-m61-prefix/installed/lib ./probe  # macOS

# 静态消费：--static 追加 Libs.private（系统库），产物自包含无需搜索路径
PKG_CONFIG_PATH=/tmp/agenterm-m61-prefix/installed/lib/pkgconfig \
  cc $(pkg-config --cflags --libs --static libagenterm) examples/c/agenterm_probe.c -o probe
./probe
```

这是里程碑 61 的唯一硬证据路径：运行时 `LD_LIBRARY_PATH` /
`DYLD_LIBRARY_PATH` 只允许指向**安装目录**（`install_consume.rs` 就是这么
做的），证明"装出来的版本化库真的能被消费者解析"。Linux 动态消费若因
rpath / 搜索路径找不到库，照实报告并给出链接器/加载器原始报错，再由人
决定是否给 `.pc` 补 `-Wl,-rpath`（那是对外合同，不经证据不改）；macOS
的 `@rpath/libagenterm.dylib` 同理——需要消费者自带 rpath，若 CI 上
动态那半跑不起来，如实贴出报错。

