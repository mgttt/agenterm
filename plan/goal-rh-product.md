# /goal — rh 做成可嵌入、可安装的动态语言产品

> 用法：在 opus0 输入 `/goal` 并贴 `--- GOAL ---` 段，或由 grok-mcu 观察进程 envelope 整段。  
> **不达目标不停。** 单 PR 做完不是收工。禁止把「先别公开仓」理解成「产品停工」。  
> SSOT：`plan/design-rh-standalone-product.md`（rev 4 + D25–D28）。

--- GOAL ---

把 **rh** 做成别人能当 bun/node/python 那样用的动态语言，而且要在诚实性上把它们比下去：失败就明说、检查器不撒谎、沙箱不能 abort 宿主、文档会被跑。**这是当前核心目标：rh 做不成，AgenTerm 的存在意义腰斩。** 武器是 TDD 验收套件，不是口号。不要推倒重写解释器。自主做到可验收；空闲就接下一条缺口，不要问「要不要停」。

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

**主线：解释器验收套件（TDD），不要重写引擎。**

1. 立一份 **Language 1 验收语料**（`crates/rh-lang/tests/accept/` 或同等）：每个 `.rh` + 期望（值 / stdout / 退出码 / 错误码）。**先提交会红的测试，再改代码到绿。** 禁止先改解释器再补测。
2. 套件必须覆盖用户路径，不只内部节点：shebang、BOM、stdin 管道、`args` 越界、沙箱、无 rustc、`check` 与 `run` 对同一源码、溢出/越界、try 抓 host 失败但不抓燃料。
3. README 的 rh / console / rust 块继续当测试跑。
4. 干净 clone + `cargo test` + `install.sh` 仍是常规闸。
5. ~~D27 native~~ 已收。Windows 只编不链。REPL / f64 / 公开仓 仍禁止。
6. 语料继续扩，优先还没钉死的用户路径：空 stdin、二进制 stdin（`read()`）、相对/绝对路径、环境变量、并发 `Engine: Send`、取消正在跑的循环、目录遍历 + catch、sort/pop/join。没有洞也要写进语料（钉住，不是包装成修复）。

TDD 纪律：发现洞 → **先写断言失败的测试（断言值或错误，不靠 exit=1 当成功）** → 再改 → 证明该测试在修前红、修后绿。

## 验证

- 在 `rh` 仓根：`cargo test -p rh-lang` 默认绿
- `cargo test -p rh-lang --features native` 在 D27 后绿
- `cargo tree -p rh-lang -e normal` 无 libloading/tempfile/agenterm
- 默认引擎 `eval` 含 `dlcall`/`enter` 必须失败

--- end GOAL ---
