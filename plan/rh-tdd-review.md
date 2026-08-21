# rh 验收语料

Sibling `rh` 仓 `crates/rh-lang/tests/accept/`。不要把家目录写进 rh 仓。

现状：135 个 `.rh`。闸：`cargo test -p rh-lang --test accept`、`cargo test -p rh-cli --test accept_cli`。非 UTF-8 管道在 `crates/rh-cli/tests/stdin_non_utf8.rs`（`// stdin:` 头只能是 UTF-8）。

语料是规格。红了改实现或改**写错的**期望；不要为了绿去迁就错误分层。没有产品洞的绿 fixture 也留着。

## 还没单独钉、值得钉的

Host：`rh::json::parse_file`、`std::fs::exists` / `metadata`、`std::time::SystemTime::now`（`.unix_millis` / `.rfc3339`）、string `push_str`、compound `*=`。越界消息现含 length，语料里还只钉 `out of range`。

库测已有、不必再写成 `.rh`：取消循环、`Engine: Send`。

PathBuf 没有 `.parent` 成员（函数是 `std::path::parent`）——已有钉，不要再当成洞。
