# JIT / AOT / 解释器：关系与联系

> 持久化时间：2026-08-10
> 触发：用户问"JIT 和 AOT 的关系和联系是怎么样的"，用 agenterm 代码库自身的例子做了解答。
> 目的：后续再问时直接引用本文，不用重答。

---

## 一句话

**AOT 和 JIT 干的是同一件事——把一种语言翻译成机器码。区别只在「什么时候翻译」。**

- AOT（Ahead-of-Time）：用户运行程序**之前**翻译好
- JIT（Just-in-Time）：用户运行程序**过程中**才翻译
- 解释器：不产机器码，维护一个 opcode 循环，读一条执行一条

三者的终点完全一样（CPU 执行指令），区别只在翻译时机和粒度。

---

## 在 agenterm 代码库中的具体例子

### Rhai AOT（`crates/agenterm-rh/src/transpile.rs`）

```
.rh 源码 → transpile.rs → .rs 源码 → rustc → 机器码 → 塞进 .exe 数据段
                                                  │
                                                  ▼
                                         用户运行 agenterm.exe 时
                                         这段机器码已经在内存里
                                         CPU 直接跳过去执行
```

翻译发生在**编译期**（`cargo build` 的时候），运行时零翻译开销。代价是改脚本要重编译。

### wasmcore JIT（`crates/agenterm-wasmcore/src/lib.rs`，wasmtime + Cranelift）

```
.wasm 字节码 → Cranelift JIT → 机器码（写到可执行内存页）→ CPU 跳过去执行
                                        │
                                        ▼
                               下次再调同一个函数 → 直接执行，不再翻译
```

翻译发生在**运行期**（`func.call()` 首次调用时），且是**按需的**——不调到的函数不编译。
代价是首次调用有编译延迟（毫秒级），但第二次调用就零开销了。

### wasmcore 的 optional precompile（AOT 路径）

```rust
// 也可以提前全编译：这本质上是「在 JIT 引擎上做 AOT」
engine.precompile_module(&module)?;
```

---

## 连续谱，不是二选一

```
纯解释执行 ──── JIT ──── 混合 AOT+JIT ──── 纯 AOT
   │              │            │               │
  每执行一句      首次调用时    热点函数AOT     全部提前
  就翻译一句      当场编译      冷函数JIT       编译好
```

agenterm 当前已有五种活跃引擎，覆盖三种翻译模型：

| 引擎 | 翻译方式 | 底层 | 用途 |
|------|---------|------|------|
| agenterm-rh | **AOT** | transpile `.rh` → `.rs` → rustc → 机器码 | 构建脚本、CI 门禁 |
| agenterm-wasmcore | **JIT** | wasmtime + Cranelift | WASM 扩展（沙箱隔离） |
| agenterm-lua | **JIT** | mlua + LuaJIT（tracing JIT） | Lua 脚本 |
| agenterm-qjs | 解释器 | rquickjs → QuickJS | JavaScript 脚本 |
| agenterm-sql | 解释器 | sqlparser → SQLite VDBE | SQL 查询 |

（agenterm-dynacore / agenterm-nativecore / agenterm-guestcore 已归档，不在 workspace）

---

## 常见混淆点

### 混淆 1：AOT 编译 vs 编译成 .exe

Rhai AOT 是在 Rust AOT 上面又叠了一层 AOT：
`.rh` → `.rs` → rustc → 机器码。最终仍是走 Rust 的 AOT 链条。

### 混淆 2：JIT ≠ 解释器

JIT 的产出是**真正的机器码**，和 AOT 产出的没有区别。
真正的解释器（CPython、老版 QuickJS）不发机器码，只维护 opcode 循环。

### 混淆 3：AOT 不一定总更快

- **峰值性能**：AOT 通常赢（编译器有无限时间做优化）
- **启动性能**：JIT 可能赢（先启动，后台慢慢编译）
- **总耗时**：JIT 可能赢（程序只跑一小段就退出时，JIT 只编译了用到的那部分）

---

## 与架构的关系

本 repo 的 `plan/plan-ape-thin-shell-dynamic-packages.md` 中，
"动态包"（`.dll`/`.so` 插件）和 AOT/JIT 是**正交概念**：

- 动态包解决的是**链接时机**（编译期 static-link vs 运行期 LoadLibrary）
- AOT/JIT 解决的是**翻译时机**（编译期翻译完 vs 运行期按需翻译）

一个动态包可以是 AOT 编译的（Rhai 插件 = rustc 编译好的 `.dll`），
也可以是 JIT 执行的（wasmcore 插件 = 运行时 Cranelift 翻译）。
