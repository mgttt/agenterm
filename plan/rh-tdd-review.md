# rh TDD 旁路审查

语料：sibling `rh` 仓 `crates/rh-lang/tests/accept/`。不要把家目录写进 rh 仓。

## 刚才补上的 fixture（应已绿）

sort / pop / remove / Map.remove / Map 内容相等 / 空数组 join / 空管道 / PATH 环境 / `-(a+3)` / 混类型 sort 报错 / 空 pop 报错。

82de7a7（34 个文件，只 pathspec）: if/else、while、for 范围/数组/空数组、break/continue、`&& || !`、整数比较含 `==`、`+=`、`insert`、数组 `get`/`contains`/`+`/`==`、负下标、字面量 `push`、map 缺键与 `contains`、字符串下标/replace/split/starts_with、json roundtrip/null/`parse` 坏文本、sha256 空串、path.join 的 `.display`、`process::id`、bytes.from_text、env PATH 非空、`check` 拒绝 `std::fs::not_a_real_function`、溢出可 catch、`+"a"` 拒绝。

语料现在 104 个 `.rh`。`cargo test -p rh-lang --test accept` 与 `cargo test -p rh-cli --test accept_cli` 都绿。

## 仍建议钉进 accept/（还没有单独语料）

- `read()` 非 UTF-8 管道（CLI-only）
- 取消正在跑的循环（库测有）
- `Engine: Send`（库测即可，不必 .rh）
- 相对路径写文件
- `switch`/`do`/`import` 的 subset 拒绝（库测有，accept 还没有）
- 多字节字符串下标（`"h"[0]` 有了，`"你好"[1]` 还没有）

## 纪律

红测不要改期望去迁就错误分层。没有产品洞的绿 fixture 也是钉。
