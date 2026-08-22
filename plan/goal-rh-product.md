# /goal — rh 解释器产品

SSOT：`plan/design-rh-standalone-product.md`。不达目标不停。不要把「先别公开仓」理解成「产品停工」。

--- GOAL ---

把 **rh** 做成可 shebang、可嵌入的动态语言：失败说清楚、`check` 不撒谎、沙箱不 abort 宿主、文档里的例子会跑。

**产品是解释执行。** JIT/AOT 若出现，只许当桌面底下对同一语言的优化，不是目标，也不是「强大」的定义。强大 = Language 1 能干活。不要推倒重写解释器。空闲就接下一条缺口。

## 保持绿（已站住，回归即失败）

1. 默认 `Engine::new()` / `rh` CLI 跑 Language 1（StdHost fs/process），无 rustc。
2. `rh.com` 当前格能交接同目录切片，退出码是脚本的。
3. `install.sh` / `package.sh` 解开再跑；交叉包整包都是目标格。
4. `feature = "native"` 有 W^X `enter_i64`；默认引擎无 FFI；不能出码的平台是 `unsupported("native: wx")`，永不 RWX。
5. 私仓按公仓口吻：无本机路径、无外仓票号、无另一个产品的内部名。
6. `cargo test -p rh-lang` 默认绿；`--features native` 绿；`cargo tree -p rh-lang -e normal` 无 libloading/tempfile/agenterm。默认 `eval` 含 `dlcall`/`enter` 必须失败。

## 现在做（三条赛道并行，互不等信封）

缺的是 host **名**就加名字，不是加语法。冻结面才报；不要每修一洞就停。

**A — 脚本在独立 rh 上跑（opus0）**  
用 AgenTerm 已有的 `bundle_project_source` 把靠 `import` 的 std-only 脚本摊成单文件，再用独立 `rh` 跑。语言不加 `import`，bundler 不搬进 rh 仓。Fleet/GUI/float 的跳过并点名。

**B — 两边解释器同一道诚实闸（本机可做）**  
独立 rh 上 Bool 对非 Bool 的 `==`/`!=` 已停。把同一闸抄进 `agenterm-rh` interp，修分叉，不是 git-pin 私仓。AOT/pack 先不动。

**C — 语料（grok）**  
钉还没覆盖的用户路径（`plan/rh-tdd-review.md`）。禁止为绿改期望。这是覆盖，不当产品进度。

已站住、不再当「现在做」：10 个无 import 的 std-only 脚本已在独立 rh 上核过；`has` 是 Bool；`kill_tree` / `is_reparse_point` 拒绝。

硬件墙（不并行假做）：Windows 两格、lnx-aarch64 真跑。没有真机器就停在类型检查/交叉包。

## 不要做

公开 `partnernetsoftware/rh`、crates.io、AgenTerm git-pin 私仓、`git filter-repo`、`git add -A`、REPL、f64、HTTP、闭包、把 JIT 当目标、libtcc、iOS 第七格、把 rustc 请回 `eval`/`run`。

`import` / `rh::task::sleep` / Fleet / GUI 不进 Language 1：工作台走 AgenTerm `Host`。`pack` / `qualify` / `task` 仍是 AOT。Windows 真跑是硬件墙。iOS 嵌签过名的解释器，不出码。

卡住且要改冻结面才报；不要每修一洞就停下来等信封。

--- end GOAL ---
