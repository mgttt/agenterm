# agenterm-abi (libagenterm)

C ABI 导出壳：嵌入方（agenterm / agenterm-con / agenterm-cu）与 OS 之间的
**机制**边界。仅导出 `exports.txt` 中的 `agt_*` 符号，不含产品概念。

## 构建（必须用 unwind profile）

规格 §3.8：panic 不得穿过 FFI 边界——每个导出都包了 `catch_unwind`，
这要求 `panic = "unwind"`。工作区默认 `[profile.dev]` / `[profile.release]`
均为 `panic = "abort"`，因此本 crate 显式使用专用 unwind profile；
在 abort profile 下编译会触发 `src/lib.rs` 顶部的 `compile_error!` 闸而失败
——这是预期信号，不是可以绕过的警告。

```powershell
# 交付 cdylib（release 语义，panic=unwind）→ target/abi-release/
cargo build -p agenterm-abi --profile abi-release

# 开发 / 测试（panic=unwind；同时构建 cdylib 并运行全部测试）
cargo test -p agenterm-abi --profile abi-dev

# 格式化检查（CI 闸：全 workspace 必须干净，退出码 0）
cargo fmt --all -- --check
```

任何不带 `--profile abi-*` 的 `cargo build/test -p agenterm-abi` 都会因编译期
闸失败（默认 profile 是 abort，会静默产出无围栏的库）。

## 测试

- `tests/exports_set.rs`：导出符号集与 `exports.txt` 完全一致（编译期不改 ABI）。
- `tests/dylib_load.rs`：用 `libloading` 加载真实 cdylib，调用 4 个导出并断言
  返回的 `const char*` 均为合法 NUL 结尾 C 字符串（缺陷回归闸）。找不到
  cdylib 时该测试直接失败（先执行上面的 build 命令）。
