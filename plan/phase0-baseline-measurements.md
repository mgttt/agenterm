# Phase 0 迁移前实测基线（里程碑 14）

> 本文档是 `plan/plan-v0.1.18.md` §14.6 Phase 0 判据的**"迁移前"实测基线**。
> 本轮只钉死前两条判据的"迁移前"列（独立产物预算、共享收益的静态链接侧），
> 后两条（渲染性能、行为等价）与"迁移后"列需要 con 的 dylib 消费变体，不在本轮范围。

## 0. 测量环境

- 仓库 HEAD：`6ae55378`（`test(abi): gate concurrent use and thread-local error isolation`）
- 工作树非干净（`plan/`、`prd/`、`AGENTS.md` 等有其它会话的在途修改），与本次测量无关
- 构建入口：仓库既有发布入口（`build.bat` → `scripts/bootstrap.cmd` → `rh task run build`）
- 目标：`x86_64-pc-windows-msvc`（本机 Windows）

## 1. 实测数字表

四条构建命令均在本机真实执行，字节数为命令退出后对产物文件的实际 stat。

| 产物 | 构建命令原文 | 字节数 | <= 1,048,575 B |
|------|--------------|--------|----------------|
| `libagenterm`（cdylib） | `cargo build -p agenterm-abi --profile abi-release` → `target/abi-release/agenterm_abi.dll` | 待填 | 待填 |
| `agenterm-con`（EXE） | `build.bat`（`con-release-fast` + build-std `panic-unwind,backtrace-trace-only`）→ `dist/agenterm-con.exe` | 待填 | 待填 |
| `agenterm`（主 EXE） | `cargo build --release --bin agenterm` → `target/release/agenterm.exe` | 待填 | 待填 |
| `agenterm-cu`（EXE） | `cargo build -p agenterm-cu --release` → `target/release/cu.exe` | 待填 | 待填 |

### con 与历史值的对照

`.reasonix-dispatch/phase0-baseline.json` 记录的 con 历史值为 **629,760 B**（2026-08-12 采集，`dist/agenterm-con.exe`）。
本轮实测值与历史值是否一致、差异原因，见下。

## 2. 盈亏平衡分析

### 2.1 当前形态：三个消费者各自静态链接

- `agenterm`（主 EXE）：静态链接机制代码
- `agenterm-con`：静态链接机制代码
- `agenterm-cu`：通过 **rlib 静态链接** `agenterm-abi`（`crates/agenterm-cu/Cargo.toml` 里是 path 依赖，
  代码直接 `use agenterm_abi::`），**不是 dlopen 动态消费**

静态链接下机制字节在三个产物中各带一份，总字节 = 三者之和：`待填` B。

> **重要事实（判据 2 防误读）**：`agenterm-cu` 已"接入 libagenterm"（rlib path 依赖 + 直接
> `use agenterm_abi::`）**不等于**已产生共享字节收益——静态链接下每个消费者仍各带一份机制代码。
> "接入"只是 API 层打通，不构成判据 2（共享收益）的证据。

### 2.2 迁移后的理论形态

迁到 dylib 后：`libagenterm.dll` **一份** + 三个瘦身后的消费者（各自通过 dlopen/链接消费同一份 dll）。

### 2.3 盈亏平衡点公式

设每个消费者迁移后平均减少 `S` 字节，净收益 = `3 * S - sizeof(libagenterm.dll)`。

- 实测 dll 字节数：`待填` B
- 实测阈值：`S > 待填 B`（= `dll 字节数 / 3`）才算真省
- 与 brief 给出的参考阈值 **120,320 B**（= 360,960 / 3）的差异及原因：待填

## 3. 未测项清单（诚实声明）

以下各项**本轮没有测**，且每一项都写明前置条件（全部需要 con 的 dylib 消费变体）：

| 未测项 | 前置条件 | 本轮状态 |
|--------|----------|----------|
| 渲染性能四项（16-step resize journey 的 frame / full-candidate / dirty-pixel / native-present 与静态版差异 < 5%） | con 的 dylib 变体可运行渲染旅程 | **未测** |
| 行为等价（90 单测 + 21 GUI 黑盒 + 多标签控制旅程全绿；公开 CLI/JSON 合同字节不变） | 迁移后产物（含 con dylib 变体） | **未测** |
| 迁移后各产物字节（dylib + 三个瘦身消费者） | con 的 dylib 消费变体 | **未测** |

以上均不得解读为"待定 / 大概率通过"——在 con 的 dylib 变体存在并实测之前，判据 2 的"迁移后"列、
判据 3、判据 4 全部视为未达成。

## 4. 构建结果与退出码

每条构建命令的真实退出码与失败原因（若有）见下表：

| 构建命令 | 退出码 | 备注 |
|----------|--------|------|
| `cargo build -p agenterm-abi --profile abi-release` | 待填 | |
| `build.bat`（con-release-fast + build-std） | 待填 | |
| `cargo build --release --bin agenterm` | 待填 | |
| `cargo build -p agenterm-cu --release` | 待填 | |
