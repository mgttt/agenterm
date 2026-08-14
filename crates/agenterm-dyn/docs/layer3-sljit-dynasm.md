# Layer 3 native codegen — SLJIT vs DynASM survey

> 调研备忘：为 **未来第三层**（将本 crate 同一套 intern 列表语言 `do` / `set` / `if` / `dlcall` 降到原生机器码）选型底层 JIT/汇编后端。**未实现**；`agenterm-dyn` 今日 **无 JIT**，`eval` 仍为解释执行 + `dlcall` 经 libffi。

## 范围

- **目标**：在保留现有 S-expr 表面与 `dlcall` 语义的前提下，把 intern 列表降到原生代码，降低热路径开销。
- **不在范围**：本 crate 当前行为、`eval` / native 路径、其他 crate、对外部项目的链接或依赖引入。
- **输入形态**：仍是解析后的列表（`do` / `set` / `if` / `dlcall`），不是通用 S-expr 引擎。

## 核对版本（2026-08）

| 组件 | 修订 |
|------|------|
| SLJIT | `fdb8e8ce20fea401c3cce718a8ede2bfc98fc37a` |
| DynASM（随 LuaJIT 树内） | `1edc3e52b67eaf6ce5f809be8e17d6862594b8bc` |

## 体积（x86_64 Linux，GCC 14.2，`-Os -DNDEBUG`）

| 指标 | SLJIT | DynASM（x64 编码器） |
|------|-------|----------------------|
| 活 ISA `.text` | ~59,608 B | ~2.8 KiB |
| 最小可执行（strip 后） | ~88,376 B | ~14,488 B |

**不可直接对比**：SLJIT 含可执行页分配器等完整运行时；DynASM 仅为宏汇编编码器，**不含** W^X 宿主、**不含** 两套 ISA 模板的完整链接成本。若采用 DynASM，还需自管映射、双后端维护与 `dlcall` 桥接。

## ISA 与抽象层

| | SLJIT | DynASM |
|---|-------|--------|
| x86_64 / AArch64 | 一等公民 | 一等公民（分 ISA 模板） |
| 中间表示 | **单一 LIR**，后端统一 | **按 ISA 的宏汇编模板**，无共享 LIR |
| 对我们 | 一套 lowering → 两架构 | 两套 emitter / 模板 |

## W^X 与可执行内存

**SLJIT**

- Linux / Windows 默认：**RWX** 可执行分配。
- 严格 W^X：编译期固定 `SLJIT_WX_EXECUTABLE_ALLOCATOR=1`，运行时用 `mprotect` / `VirtualProtect` 切换可写/可执行。
- Apple：MAP_JIT + `pthread_jit_write_protect_np`；应用侧仍可能需要 JIT entitlements。

**DynASM**

- **不分配**可执行页；只生成字节流。
- **不要**照搬 LuaJIT 的 `lj_mcode.c` 整段 lifted——那是完整 VM 的 mcode 管理，与 `agenterm-dyn` 边界不符；若选用需自研薄 W^X 层。

本 crate 今日 `dlcall` 走 libffi、**无可写可执行页**；第三层若引入 JIT，W^X 策略需单独设计并与现有安全叙事对齐。

## 如何接我们的 intern 列表

两者都 **不** 直接消费 S-expr / intern 列表，需要薄 lowering：

**SLJIT**

- 遍历 `do` / `set` / `if`，生成 LIR 基本块与分支。
- `dlcall`：保留 **libffi C 辅助函数**（动态 CIF、变参、库路径解析仍在 Rust/C 侧）；`sljit_emit_icall` 为 **固定签名、≤4 参数** 的便捷路径，**不能**替代完整动态 `dlcall`。
- 变量槽可用固定栈帧或环境指针。

**DynASM**

- 需 **两套** emitter（x86_64 与 AArch64 各一套 `.dasc` / 生成头）。
- 同样：`dlcall` 落点为 libffi 或固定 thunk，而非在模板里重写 ffi。

## 许可证

| 后端 | 许可 |
|------|------|
| SLJIT | 2-clause BSD |
| DynASM | MIT |

与仓库现有许可混用时需保留各自 notice（若将来 vendoring）。

## 建议

1. **第三层优先试 SLJIT**：单一 LIR、双 ISA 已验证、与「薄 lowering + libffi `dlcall`」模型契合；体积更大但集成面更窄。
2. **仅当** 团队愿意长期维护 **两套宏汇编后端**、且对编码器体积有硬上限时，再考虑 DynASM。
3. 无论选型，**不** 在本调研阶段改 `eval` / `native` 行为；落地时单独里程碑与测试矩阵（含 W^X 与双架构 smoke）。

## 与当前 crate 的关系

```
Layer 1（今日）: parse → eval（解释） + dlcall（libffi）
Layer 3（调研）: parse → lowering → JIT/asm → 原生代码；dlcall 仍经 libffi 边界
```

README 中的集成与 libagenterm 接线仍 **延后**；本文档仅沉淀选型知识，避免散落在对话里。
