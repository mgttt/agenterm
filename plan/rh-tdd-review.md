# rh 验收语料

Sibling `rh` 仓 `crates/rh-lang/tests/accept/`。不要把家目录写进 rh 仓。

现状：361 个 `.rh`。闸：`cargo test -p rh-lang --test accept`、`cargo test -p rh-cli --test accept_cli`。非 UTF-8 管道在 `crates/rh-cli/tests/stdin_non_utf8.rs`（`// stdin:` 头只能是 UTF-8）。

语料是规格。红了改实现或改**写错的**期望；不要为了绿去迁就错误分层。没有产品洞的绿 fixture 也留着。

本脉冲：bytes 无 `join`；空 `to_lower`/`trim`；`for` bool 是 parse；bytes `<` 停止；`[1][true]` parse（integer index）。

## 还没单独钉、值得钉的

`m.k =` 已在 `-=` fixture。type_of Duration 已在 timeout fixture。sha256 `"rh"` 已在 file fixture 的 `out:`。type_of FileLock 已在 lock fixture。bytes.get 越界已是 unit（在 from_array fixture）。

array 无 `split`。string 无 `keys`。`m.get(())` 是 cannot index map with ()。bytes.get(string) 是 cannot index bytes with string。`"".to_string()` 是空。

`string.push` / `int.parse_int` / `map.sort` 目前报 host unsupported（不是 `no method`）——分层洞，先报告别改解释器。别把它们钉成 host unsupported。

`output.error` 已在 deadline fixture 钉过。`continue 1` 是 parse 不是 `RH_SUBSET_BREAK_VALUE`。`throw` 无参是 runtime 不是 `RH_SUBSET_THROW_ARGS`；`1 = 2` 是 parse 不是 `RH_SUBSET_ASSIGN_LHS`。别把它们钉成 subset。

JSON `i64::MIN-1` 文案是科学计数法，别钉死字面。

库测已有、不必再写成 `.rh`：取消循环、`Engine: Send`。

PathBuf 没有 `.parent` 成员（函数是 `std::path::parent`）——已有钉，不要再当成洞。
