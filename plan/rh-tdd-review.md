# rh 验收语料

Sibling `rh` 仓 `crates/rh-lang/tests/accept/`。不要把家目录写进 rh 仓。

现状：141 个 `.rh`。闸：`cargo test -p rh-lang --test accept`、`cargo test -p rh-cli --test accept_cli`。非 UTF-8 管道在 `crates/rh-cli/tests/stdin_non_utf8.rs`（`// stdin:` 头只能是 UTF-8）。

语料是规格。红了改实现或改**写错的**期望；不要为了绿去迁就错误分层。没有产品洞的绿 fixture 也留着。

## 还没单独钉、值得钉的

字节 256 必须拒绝（不 wrap）；JSON 里装不下的数必须拒绝；host 错 arity（`expects N argument(s)`）；compound `/=` `%=`。

库测已有、不必再写成 `.rh`：取消循环、`Engine: Send`。

PathBuf 没有 `.parent` 成员（函数是 `std::path::parent`）——已有钉，不要再当成洞。
