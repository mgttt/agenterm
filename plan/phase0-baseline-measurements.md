# Phase 0 迁移前实测基线（里程碑 14）

> Historical snapshot: this document preserves the artifact names, paths,
> hashes and linkage observations measured at that time. The row named
> `target/release/cu.exe` is the old binary path and its hash meaning must not be
> rewritten. Current product truth is one executable named `agenterm-cu`, with
> CLI and host modes in that binary, and CU as the first runtime
> `libagenterm` consumer.
> 本文档是 `plan/plan-v0.1.18.md` §14.6 Phase 0 判据的**"迁移前"实测基线**。
> 本轮只钉死前两条判据的"迁移前"列（独立产物预算、共享收益的静态链接侧），
> 后两条（渲染性能、行为等价）与"迁移后"列需要 con 的 dylib 消费变体，不在本轮范围。
>
> **后续更新（里程碑 23）**：判据 2 已由 `examples/size-probe/` 拿到探针级负面证据
> （结论档位「档 2：很可能不成立」，待 con 迁移实测确认），数字与结论见 §2.4；
> 判据 3、判据 4 维持"未测"不变。
>
> **后续更新（里程碑 60）**：判据 3 的**"迁移前"列**（静态版 16-step resize journey
> 的 frame / full-candidate / dirty-pixel / native-present 四项）已实测，数字与离散
> 程度见 §3；"迁移后"列仍需 con 的 dylib 消费变体，本轮未做，**判据 3 仍未结论**。
> 判据 4 维持"未测"。

## 0. 测量环境

- 仓库 HEAD：`6ae55378`（`test(abi): gate concurrent use and thread-local error isolation`）
- 工作树非干净（`plan/`、`prd/`、`AGENTS.md` 等有其它会话的在途修改），与本次测量无关
- 构建入口：仓库既有发布入口（`build.bat` → `scripts/bootstrap.cmd` → `rh task run build`）
- 目标：`x86_64-pc-windows-msvc`（本机 Windows）
- 每条构建命令都真实执行；字节数取自命令退出后对产物文件的实际 `stat`/`sha256sum`

## 1. 实测数字表

| 产物 | 构建命令原文 | 字节数 | <= 1,048,575 B |
|------|--------------|--------|----------------|
| `libagenterm`（cdylib） | `cargo build -p agenterm-abi --profile abi-release` → `target/abi-release/agenterm_abi.dll` | **400,896** | **是** |
| `agenterm-con`（EXE） | `build.bat`（`con-release-fast` + build-std `panic-unwind,backtrace-trace-only`）→ `dist/agenterm-con.exe` | **629,760** | **是** |
| `agenterm`（主 EXE） | `cargo build --release --bin agenterm` → `target/release/agenterm.exe` | **4,224,512** | **否**（判据 1 不约束主 EXE，仅报告） |
| `agenterm-cu`（EXE） | `cargo build -p agenterm-cu --release` → `target/release/cu.exe`（历史路径；现正式名为 `agenterm-cu`） | **351,232** | **是**（判据 1 不约束 cu，仅报告） |

> 判据 1（独立产物预算）只约束 `libagenterm.{dll,so,dylib}` 与迁移后的 con EXE。
> 主 EXE 与 cu 的 `<= 1,048,575 B` 列仅如实报告，不代表它们承诺满足该预算。

### 1.1 con 与历史值的对照

`.reasonix-dispatch/phase0-baseline.json` 记录的 con 历史值为 **629,760 B**
（2026-08-12 采集，`dist/agenterm-con.exe`，HEAD `f628b0e7`）。

- 本轮实测 **629,760 B**，与历史值**字节数完全一致**。
- 注意：期间 con 源码已迁入自有包 `crates/agenterm-con`
  （`02f3630b refactor(con): move source into owning package`），HEAD 也从 `f628b0e7`
  推进到 `6ae55378`；字节数不受影响，说明 con 二进制尺寸稳定。
- 二进制哈希不同（历史 `8802EFF7...`，本轮 `842bf552...`），差异来自嵌入的 build
  identity（git commit / 工作树 dirty 状态），不影响产物尺寸。
- 构建通道说明：首轮以默认通道（cargo stderr 直接继承）运行 `build.bat` 出现
  `build_cargo:exit=101`，stderr 因宿主 GUI 进程句柄问题不可见；以 `CI=1` 通道
  （build.rh 内仅切换 cargo stderr 捕获方式，编译内容与产物语义不变）重跑成功，
  四条产物均由此成功运行的官方入口产出。

## 2. 盈亏平衡分析

### 2.1 当前形态：三个消费者各自静态链接

| 消费者 | 静态链接机制代码方式 |
|--------|----------------------|
| `agenterm`（主 EXE） | 普通 crate 静态链接 |
| `agenterm-con` | 普通 crate 静态链接 |
| `agenterm-cu` | 通过 **rlib 静态链接** `agenterm-abi`（`crates/agenterm-cu/Cargo.toml` 为 path 依赖，代码直接 `use agenterm_abi::`） |

静态链接下机制字节在三个产物中各带一份：

**总字节 = 4,224,512 + 629,760 + 351,232 = 5,205,504 B**

> **重要事实（判据 2 防误读）**：`agenterm-cu` 已"接入 libagenterm"（rlib path 依赖 +
> 直接 `use agenterm_abi::`）**不等于**已产生共享字节收益——静态链接下每个消费者仍各带
> 一份机制代码。"接入"只是 API 层打通，不构成判据 2（共享收益）的证据。判据 2 的证据
> 现状见 §2.4：`examples/size-probe/` 已给出探针级负面结论（很可能不成立，待实测
> 确认）；"迁移后"（一份 dylib + 三个瘦身消费者）密封总字节的权威实测仍需 con 的
> dylib 变体。

### 2.2 迁移后的理论形态

迁到 dylib 后：`libagenterm.dll` **一份** + 三个瘦身后的消费者（各自 dlopen/链接同一份 dll）。
共享收益 = 三个消费者迁移后减少的字节之和，减去新增的 dll 体积。

### 2.3 盈亏平衡点公式（按实测重算）

设每个消费者迁移后平均减少 `S` 字节，则：

```
净收益 = 3 * S - sizeof(libagenterm.dll)
```

- 实测 dll 字节数：**400,896 B**
- 实测盈亏平衡阈值：**S > 400,896 / 3 = 133,632 B**
- 即：每个消费者平均要瘦掉 **> 133,632 B**（三份合计砍掉 > 400,896 B）才抵消一份
  dll 的成本，才是真省。
- 与 brief 参考阈值 **120,320 B**（= 360,960 / 3）比较：实测 dll 比参考口径大
  39,936 B，阈值相应抬高 **13,312 B**。参考值不是实测，以本表实测阈值为准。

参考量级：主 EXE 4,224,512 B、con 629,760 B、cu 351,232 B。每个消费者要砍
>133.6 KB 的机制字节，意味着当前静态链接的机制代码在该消费者中必须显著大于
133.6 KB（三消费者各不相同，最终以"迁移后"实测为准）。

### 2.4 判据 2 的探针证据（size-probe，里程碑 15 + 23 + 34）

在 con 迁移之前，`examples/size-probe/` 用最小双变体探针测"消费者迁到动态 dylib
后能瘦多少"（记作 `S_probe`），以提前判决判据 2（共享收益）是否可能成立。
完整证据（变体说明、构建命令、运行输出、PE 导入表佐证）见
[`examples/size-probe/README.md`](../examples/size-probe/README.md)，本表只收录其数字：

| 口径 | 变体 A（静态） | 变体 B（动态 dylib） | `S_probe` |
|------|----------------|----------------------|-----------|
| 里程碑 15（4 组机制：runtime / clipboard / process / parent-console） | 132,608 B | 147,456 B | **−14,848 B** |
| 里程碑 23（6 组机制，补 window + pty） | 238,592 B | 151,552 B | **+87,040 B** |
| 里程碑 34（全量，+screenshot 双导出 +a11y +event_text） | 251,392 B | 164,352 B | **+87,040 B** |
| window+pty 增量贡献（同为 `--release` 实测，可比） | +105,984 B | +4,096 B | **+101,888 B** |

与 §2.3 实测盈亏平衡阈值 **133,632 B** 对照：

- `S_probe`（+87,040 B）**小于**阈值，缺口 **46,592 B**（仅为阈值的 **65%**）；
- window 与 pty 是 con 中公认最大的两块机制，合计才为 `S_probe` 贡献约 +101,888 B；
- **剩余机制对 `S_probe` 的贡献是实测 +0 B**（M34 − M23，同为 `--release` 实测，
  可比）：变体 A 与变体 B 各同增 12,800 B——A 是截图编码 + 窗口捕获后端 + a11y stub +
  IME 匹配的静态成本，B 是新增导出解析与探测胶水——两者恰好抵消，`S_probe` 自里程碑 23
  起停在 +87,040 B 一分未涨。剩余机制（screenshot / a11y / event_text，以及体积占比
  更小于它们的 filesystem / ipc / 其余 input·ime 路径）的静态成本与变体 B 的 FFI 胶水
  相当，很可能因 window/pty 已把它们传递性地链进来，要补上 46,592 B 缺口没有空间。
  上一轮"剩余机制补不上缺口"是**推断**，本轮已是**实测结论**；
- 结论档位（size-probe README 的三档之一）：**「档 2：判据 2 很可能不成立」**，
  且这是**不需要先迁 con** 就能拿到的负面结论；
- **档 3（不足以判决）已被排除**：M15 说不足以判决是因为 window+pty 未测，M23 说
  不足是因为剩余机制未测；M34 把剩余机制测完，`S_probe` 仍停在 +87,040 B（阈值的
  65%，缺口 46,592 B），两个"未测"理由都已消除，三档里只剩档 2。

**估算的性质与边界**（沿用 size-probe README 口径，非 con 迁移实测）：

- 这是**下界估算**：真实 con 链进的 window/pty 机制比最小探针多，探针测得的
  `S_probe` 不高于真实 `S`；
- **不是** con 迁移实测的替代品：判据 2 的"迁移后"列（一份 dylib + 三个瘦身消费者
  的密封总字节）仍需 con 的 dylib 消费变体实测才能得到权威结论；
- 但它足以支撑"继续投入前先做决策"这一判断。

**给决策者的事实陈述**（陈述证据状态与规则原文，不构成建议或推荐）：
`plan/plan-v0.1.18.md` §14.6 的规则是"判据不过 → 本节整体删除并在 §9 决策记录留
一行否决理由与实测数字，不留残叶"。本文档只陈述判据 2 目前处于什么证据状态
（探针结论「档 2：很可能不成立」，待实测确认）；是否触发该规则由人决定。

## 3. 渲染性能四项（判据 3 的"迁移前"列，里程碑 60）

> 本节是判据 3 的**"迁移前"列**：静态版（现存的 con 静态链接形态）自己跑
> 16-step resize journey 的四项计数器实测。**"迁移后"列需要 con 的 dylib 消费变体，
> 本轮未做，判据 3 仍未结论**。本节只测量与记录，不下"通过 / 不通过"结论。

### 3.1 测量环境

- 仓库 HEAD：`118a6e04`（`docs(abi): state the one-link-form rule the probe just measured`）
- 工作树干净（测量前 `git status` 无变更），二进制为当前 HEAD 源码增量重建
- 构建入口：`cargo build -p agenterm-con --bin agenterm-con`（dev profile，增量命中）
- 测试入口：`cargo test -p agenterm-con --test agenterm_con_blackbox controlled_resize_storm -- --nocapture`
- 目标：`x86_64-pc-windows-msvc`（本机 Windows，真实桌面，非 Wine）
- journey 即现成测试 `controlled_resize_storm_reports_successful_frames_and_exits_cleanly`
  （`crates/agenterm-con/tests/agenterm_con_blackbox.rs`），**未新写 journey**：
  - 4 轮 × 4 个尺寸 `(720,460) (1180,740) (640,420) (1100,680)` = **正好 16 步 resize**；
  - 开头 `{"reset_perf":true}`，结尾 `{"perf_stats":<path>}` 把计数器写成 `perf.json`；
  - Windows 上带 `native_resize_phase` 的 begin/end；
  - `perf.json` 由测试写在 `%TEMP%` 下的独立 scratch 目录，测试结束不删除目录；
    三次实测后原样保留在临时目录并逐字抄录于 §3.2。

### 3.2 三次实测的 `perf.json` 完整内容（原样，未摘要）

**第 1 次**：

```json
{
  "frames": 3,
  "observed_frames": 3,
  "render_last_us": 13491,
  "render_average_us": 11428,
  "render_max_us": 14240,
  "pty_drained_bytes": 0,
  "pty_budget_yields": 0,
  "control_requests": 18,
  "control_budget_yields": 0,
  "full_candidate_frames": 2,
  "partial_candidate_frames": 1,
  "dirty_pixels": 1496000,
  "frame_pixels": 2244000,
  "host_direct_frames": 3,
  "host_copy_frames": 0,
  "host_copy_pixels": 0,
  "discarded_capture_frames": 0,
  "present_count": 12,
  "present_success": 12,
  "present_failure": 0,
  "last_ns": 692500,
  "total_ns": 13637900,
  "max_ns": 1094800,
  "full_pixels": 9476800,
  "partial_pixels": 0,
  "requested_full_pixels": 9476800,
  "requested_partial_pixels": 0
}
```

**第 2 次**：

```json
{
  "frames": 3,
  "observed_frames": 3,
  "render_last_us": 14367,
  "render_average_us": 11398,
  "render_max_us": 14367,
  "pty_drained_bytes": 0,
  "pty_budget_yields": 0,
  "control_requests": 18,
  "control_budget_yields": 0,
  "full_candidate_frames": 2,
  "partial_candidate_frames": 1,
  "dirty_pixels": 1496000,
  "frame_pixels": 2244000,
  "host_direct_frames": 3,
  "host_copy_frames": 0,
  "host_copy_pixels": 0,
  "discarded_capture_frames": 0,
  "present_count": 12,
  "present_success": 12,
  "present_failure": 0,
  "last_ns": 736300,
  "total_ns": 14678200,
  "max_ns": 1452100,
  "full_pixels": 9060000,
  "partial_pixels": 0,
  "requested_full_pixels": 9060000,
  "requested_partial_pixels": 0
}
```

**第 3 次**：

```json
{
  "frames": 3,
  "observed_frames": 3,
  "render_last_us": 6562,
  "render_average_us": 8856,
  "render_max_us": 13975,
  "pty_drained_bytes": 0,
  "pty_budget_yields": 0,
  "control_requests": 18,
  "control_budget_yields": 0,
  "full_candidate_frames": 1,
  "partial_candidate_frames": 2,
  "dirty_pixels": 748000,
  "frame_pixels": 2025584,
  "host_direct_frames": 3,
  "host_copy_frames": 0,
  "host_copy_pixels": 0,
  "discarded_capture_frames": 0,
  "present_count": 12,
  "present_success": 12,
  "present_failure": 0,
  "last_ns": 573000,
  "total_ns": 14413500,
  "max_ns": 1145100,
  "full_pixels": 8841584,
  "partial_pixels": 0,
  "requested_full_pixels": 8841584,
  "requested_partial_pixels": 0
}
```

### 3.3 四项计数：三次实测值与离散程度

判据 3 的"渲染性能"阈值针对四项：**frame / full-candidate / dirty-pixel /
native-present**。对应本 journey 的计数器如下表。

| 计数（判据 3 对应项） | 第 1 次 | 第 2 次 | 第 3 次 | 极差 | 相对离散（极差 / 中位数） |
|---|---|---|---|---|---|
| `frames`（frame） | 3 | 3 | 3 | 0 | 0% |
| `full_candidate_frames`（full-candidate） | 2 | 2 | 1 | 1 | **50%** |
| `dirty_pixels`（dirty-pixel） | 1,496,000 | 1,496,000 | 748,000 | 748,000 | **50%** |
| `host_direct_frames`（native-present） | 3 | 3 | 3 | 0 | 0% |
| `host_copy_frames`（native-present 副本） | 0 | 0 | 0 | 0 | — |

对照参考（同文件其他字段，帮助判断稳定性）：
`partial_candidate_frames` 为 1 / 1 / 2（与 full-candidate 互补，总数恒为 3）；
`frame_pixels` 为 2,244,000 / 2,244,000 / 2,025,584（第 3 次偏小，与
full-candidate 少 1 帧一致）；`present_count` / `present_success` 恒为 12 / 12，
`present_failure` 恒为 0。

### 3.4 阈值可判定性：同一二进制自比已远超 5%

**同一二进制（当前 HEAD 静态版）自比**。前三次由本节 §3.2 记录；
**第 4 次为独立复核**（同一 HEAD、同一命令，另一个会话执行），
结果推翻了三次样本给出的"零波动"印象：

| 计数 | 第 1 次 | 第 2 次 | 第 3 次 | **第 4 次（复核）** | 极差 / 中位数 |
|---|---|---|---|---|---|
| `frames` | 3 | 3 | 3 | **2** | **33%** |
| `full_candidate_frames` | 2 | 2 | 1 | **1** | **50%** |
| `dirty_pixels` | 1,496,000 | 1,496,000 | 748,000 | **748,000** | **50%** |
| `host_direct_frames` | 3 | 3 | 3 | **2** | **33%** |
| `host_copy_frames` | 0 | 0 | 0 | **0** | 0（零拷贝不变式成立） |

**四项全部不可判定**：自比波动 33%–50%，比 5% 的对照阈值高一个量级。

根因是**样本量**，不是渲染路径不稳：这条 16 步 journey 全程只产生 **2–3 帧**。
帧数如此之少时，任何一帧在 full-candidate / partial-candidate 之间翻转，
就会让指标整体移动 33%–50%；`dirty_pixels` 与帧的分类直接耦合，所以同步跳变。
5% 的分辨率要求与 n=3 的量化粒度在数量级上不匹配。

**方法论注记**：前三次一致曾让 `frames` 与 native-present 看起来"零波动"，
第 4 次即推翻。**三次样本不足以断言稳定性**，这一点本身也记录在案。

**为什么样本量提不上去（实测，非推断）**：最初的猜测是竞态 —— 16 个
`resize-window` 控制命令背靠背发出，journey 里没有任何等待帧的同步，
所以怀疑计数器在帧落地前就被 dump 了。**这个猜测是错的。**
临时在每次 resize 后插入 `{"wait_ms":80}`（16 次共多出 1.28 秒沉降），
连跑三次，帧数**纹丝不动**：

| 变体 | frames | full-candidate | dirty_pixels |
|---|---|---|---|
| 原样（4 次） | 3 / 3 / 3 / 2 | 2 / 2 / 1 / 1 | 1,496k / 1,496k / 748k / 748k |
| 每步 +80ms（3 次） | 2 / 2 / 3 | 1 / 1 / 1 | 748k / 748k / 748k |

（实验用的临时改动跑完即还原，文件 SHA256 逐位核对；仓库未留痕。）

根因因此不是节奏，而是**这条 journey 不产生渲染工作**：三次运行的
`pty_drained_bytes` 全为 0，终端内容自始至终没有变化，改变窗口大小只触发了
极少数重绘。**慢下来并不会多出帧**，要提高样本量必须让它真的渲染
（PTY 有持续输出）或改用另一条 journey。

结论（如实陈述，不修改任何阈值）：**判据 3 的"与静态版差异 < 5%"目前不可判定**
—— 不是因为迁移会不会变慢，而是因为这条 journey 的输出粒度撑不起 5% 的分辨率。
在 journey 或指标改变之前，即使建出 con 的 dylib 变体也判不出结果 ——
**这一条直接影响"要不要投入做 dylib 变体"的排序**。

可选方向（供决策，本轮不实施、不擅改 `plan/plan-v0.1.18.md`）：

1. **让 journey 真的渲染**：resize 期间保持 PTY 有输出，帧数上去后量化
   粒度自然变细；代价是 journey 的语义从"纯 resize"变成"resize + 内容churn"。
2. **重复整条 journey 取分布**：跑 N 遍比较中位数/置信区间而非单次值；
   代价是 GUI 测试时长线性增长。
3. **换指标**：改用对帧分类不敏感的量（例如每帧平均渲染耗时
   `render_average_us`、或 `total_ns/present_count`），避开
   full-candidate/partial-candidate 翻转造成的整段跳变。
4. **修订阈值**：承认这条 journey 只能给出粗判，把 5% 放宽到与量化粒度
   相称的水平。

我的倾向是 1 + 3 组合：先让样本量足够，再用不依赖帧分类的指标，
两者都不改变判据"比较迁移前后渲染代价"的本意。但这是排期与规格问题，
由人拍板。

### 3.5 边界声明（防误读）

- 本节数字是**静态版自己**的"迁移前"列，不是"迁移前 vs 迁移后"的对照；
- "迁移后"列需要 con 的 dylib 消费变体（§14 的"另建 dylib 变体"），**本轮未做**；
- **判据 3 仍未结论**：本节不给出"通过 / 不通过"判断，只提供"迁移前"基线与
  自比离散度，供后续 dylib 变体对照时使用。

## 4. 未测项清单（诚实声明）

以下各项**本轮没有测**，且每项都写明前置条件（全部需要 con 的 dylib 消费变体）：

| 未测项 | 判据 | 前置条件 | 本轮状态 |
|--------|------|----------|----------|
| 渲染性能四项：16-step resize journey 的 frame / full-candidate / dirty-pixel / native-present 与静态版差异 < 5% | 判据 3 | con 的 dylib 变体可运行渲染旅程 | **"迁移前"列已测（里程碑 60，见 §3）；"迁移后"列未测，判据 3 仍未结论** |
| 行为等价：90 单测 + 21 GUI 黑盒 + 多标签控制旅程全绿；公开 CLI/JSON 合同字节不变 | 判据 4 | 迁移后产物（含 con dylib 变体） | **未测** |
| 迁移后各产物字节（一份 dylib + 三个瘦身消费者，密封总字节） | 判据 2 "迁移后"列 | con 的 dylib 消费变体 | **有负面证据、待实测确认**（探针结论见 §2.4，完整证据见 `examples/size-probe/README.md`） |
| `libagenterm.{so,dylib}` 跨平台产物尺寸 | 判据 1 全平台列 | 本机为 Windows，仅测了 `dll` | **未测** |

判据 3、判据 4 维持**未结论**：判据 3 的"迁移前"列数字见 §3，但"迁移后"列在
con 的 dylib 变体存在并实测之前仍缺失，判据 3 视为未达成；判据 4 仍完全未测。
两者都不得解读为"待定 / 大概率通过"。
判据 2 的"迁移后"列已从"完全未测"移到**有负面证据、待实测确认**：§2.4 的
size-probe 探针结论为「档 2：很可能不成立」，足以支撑"继续投入前先做决策"，
但不能替代 con 迁移实测的权威结论；"迁移后"密封总字节仍待实测。

## 5. 构建结果与退出码

| 构建命令 | 退出码 | 备注 |
|----------|--------|------|
| `cargo build -p agenterm-abi --profile abi-release` | **0** | 增量命中，产物 400,896 B |
| `build.bat`（con-release-fast + build-std） | **0** | 首轮默认通道失败（`build_cargo:exit=101`，stderr 因宿主 GUI 句柄不可见），`CI=1` 通道重跑成功；`CI` 仅切换 cargo stderr 捕获方式，不改变编译内容 |
| `cargo build --release --bin agenterm` | **0** | 冷编译 3m44s，产物 4,224,512 B |
| `cargo build -p agenterm-cu --release` | **0** | 产物 351,232 B |

产物哈希（本轮实测，SHA-256）：

| 产物 | SHA-256 |
|------|---------|
| `target/abi-release/agenterm_abi.dll` | `99d9119c931ae6b4101acc227542a5d2903a09fe38bab2070e9d1397230dc374` |
| `dist/agenterm-con.exe` | `842bf55235fe8971e06f5462fbe1f8121bf10a858a4e1b27c4a05683a18df9c4` |
| `target/release/agenterm.exe` | `bd32dbe5d7e7e8cd0b0e40678da9aadb2c87e49bbf950c353f98b2ab92184b2e` |
| `target/release/cu.exe`（历史路径；保留原测量含义） | `55aa35c967223b18819bf63fbfe5fc102e05b630a840a4c416ed78d86d36cd63` |
