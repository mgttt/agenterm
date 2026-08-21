# /goal — rh 做成可嵌入、可安装的动态语言产品

> 用法：在 opus0 输入 `/goal` 并贴 `--- GOAL ---` 段，或由 grok-mcu 观察进程 envelope 整段。  
> **不达目标不停。** 单 PR 做完不是收工。禁止把「先别公开仓」理解成「产品停工」。  
> SSOT：`plan/design-rh-standalone-product.md`（rev 6，D38–D39）。

--- GOAL ---

把 **rh** 做成别人能当 bun/node/python 那样用的动态语言，而且要在诚实性上把它们比下去：失败就明说、检查器不撒谎、沙箱不能 abort 宿主、文档会被跑。**产品是解释执行的能力，不是 JIT/AOT。** 桌面以后可以在同一套语言底下做底层优化；那不是这几程要追求的，也不是「强大」的定义。强大 = Language 1 能干活（Host、诚实报错、check 不撒谎、沙箱不炸宿主、语料）。不要推倒重写解释器。自主做到可验收；空闲就接下一条缺口，不要问「要不要停」。

## 完成条件（/goal 判定，全绿才算）

1. 默认 `Engine::new()` / `rh` CLI 能跑 Language 1（含 StdHost fs/process）；无 rustc。
2. `rh.com` 对当前格能 exec 同目录切片，退出码是脚本的。
3. `install.sh` / `package.sh` 当前格往返绿。
4. **D27：** `feature = "native"` 有 W^X CodeBuffer + `enter_i64`；默认引擎仍无 FFI；无 JIT 的平台返回 `unsupported("native: wx")`，永不 RWX。
5. 私仓注释/提交按 **D28**（公仓口吻）；无 mux 名、无 PR-A1 票号、无本机路径。
6. **仍禁止**（即使完成条件 1–5 已绿也不做，除非董事长新令）：公开 `partnernetsoftware/rh`、crates.io、AgenTerm `git pin` rh.git、`git filter-repo`。

未完成就继续。一条 PR 的「完成」只是内部里程碑。

## 边界

| 做 | 不做 |
|---|---|
| 与 AgenTerm 并列的 `rh` 工作树（语言/CLI/loader/native） | 把 dyn 的 S 表达式并进 rh |
| AgenTerm 仅当工作台要接解释器/文档 | 改 tinyvm、platform 大修、prd 扩写汇编小说 |
| squash/copy、pathspec 提交 | `git add -A`、subtree、filter-repo |
| 卡住且要改冻结面才报 grok-mcu | 每修一洞就停下来等信封（会打断输入） |

## 当前缺口（按序啃，做完划掉）

**主线仍是 TDD，不要重写引擎。核已经能站住；下一程是拿真实脚本去撞 Language 1，看它停在哪，而不是加语法去像 bun。**

1. ~~立验收语料~~ `crates/rh-lang/tests/accept/` 已有（117+）。继续钉还没单独覆盖的 host 名；禁止为了绿改期望。
2. ~~用户路径骨架~~ shebang / BOM / stdin（含非 UTF-8）/ args / 沙箱 / 无 rustc / check↔run / 溢出 / try 不抓燃料 — 已钉。
3. README 的 rh / console / rust 块继续当测试跑（依赖列表、性能形状、D28 脱敏已开始守）。
4. 干净 clone + `cargo test` + `install.sh` / 解开包再跑 / 交叉整包目标格 — 常规闸。
5. ~~D27 native~~ 已收。Windows 只编不链。REPL / f64 / 公开仓 / HTTP / 闭包 / 把 JIT 当目标 仍禁止。
6. **下一程（用真实脚本量解释器天花板，先写测试再改）。D39：不要把时间花在出码上。**
   - 把 AgenTerm 里只依赖 `std`/`rh::json`/`command_status` 的脚本（如 `internal-version-policy.rh` 这一类）在独立 `rh` 上跑通。AOT 时代的 `bool == 0` 在 Language 1 里不是假：改脚本，不要加强制转换。
   - `import` / `rh::task::sleep` / Fleet / GUI 子进程 **不进 Language 1**。工作台脚本继续走 AgenTerm `Host` 注入。发现缺的是 host 名就加名字，不是加语法。
   - `task`/`qualify`/`pack` 仍是 AOT；`eval`/`run` 已是解释器。不要把 rustc 请回默认路径。
   - Windows 真跑是下一堵硬件墙，不是这程的语言墙。
   - **iOS（D38）：** 能走的是 Pyto 那条 — 签过名的解释器进 App，`.rh` 当数据；不是现场出码。native 门保持 `unsupported("native: wx")`。不上 libtcc，不编第七格，不把 LLVM bitcode 解释器当 v1。纯脚本库可以后加（类似纯 Python 的 pip）；带原生扩展的只能预编译进包再签。

TDD 纪律：发现洞 → **先写断言失败的测试（断言值或错误，不靠 exit=1 当成功）** → 再改 → 证明该测试在修前红、修后绿。

## 验证

- 在 `rh` 仓根：`cargo test -p rh-lang` 默认绿
- `cargo test -p rh-lang --features native` 在 D27 后绿
- `cargo tree -p rh-lang -e normal` 无 libloading/tempfile/agenterm
- 默认引擎 `eval` 含 `dlcall`/`enter` 必须失败

--- end GOAL ---
