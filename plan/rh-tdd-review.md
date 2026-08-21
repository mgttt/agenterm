# rh 验收语料

Sibling `rh` 仓 `crates/rh-lang/tests/accept/`。不要把家目录写进 rh 仓。

现状：199 个 `.rh`。闸：`cargo test -p rh-lang --test accept`、`cargo test -p rh-cli --test accept_cli`。非 UTF-8 管道在 `crates/rh-cli/tests/stdin_non_utf8.rs`（`// stdin:` 头只能是 UTF-8）。

语料是规格。红了改实现或改**写错的**期望；不要为了绿去迁就错误分层。没有产品洞的绿 fixture 也留着。

本脉冲：`create_dir` 拒缺父目录、DirEntry `.path`/`.is_dir`、`std::process::list` 运行时 unsupported（check 放行）、`command_status` 省略参数列与 1–2 元数、`env::get` 缺名是空串。

## 还没单独钉、值得钉的

Child `.stdout`/`.stderr` 在 wait 前；`try_rename` 成功路径；`exists_case_exact` 对刚写入的文件。

`output.error` 已在 deadline fixture 钉过。`continue 1` 是 parse 不是 `RH_SUBSET_BREAK_VALUE`。`throw` 无参是 runtime 不是 `RH_SUBSET_THROW_ARGS`；`1 = 2` 是 parse 不是 `RH_SUBSET_ASSIGN_LHS`。别把它们钉成 subset。

JSON `i64::MIN-1` 文案是科学计数法，别钉死字面。

库测已有、不必再写成 `.rh`：取消循环、`Engine: Send`。

PathBuf 没有 `.parent` 成员（函数是 `std::path::parent`）——已有钉，不要再当成洞。
