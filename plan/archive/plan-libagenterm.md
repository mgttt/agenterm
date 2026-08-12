> ## ⚠️ 已归档（2026-08-12）
>
> 本专题**未立项、未开工**。其完整内容已全文合并至在制版本
> [`plan-v0.1.18.md`](../plan-v0.1.18.md) §14 轨 E（`libagenterm` 机制库），
> 本文件只保留追溯价值，**不得重新作为活跃 SSOT**。
>
> ---
>
# `libagenterm.{so,dylib,dll}` — 机制层动态库

状态：**已接受规划、尚未实现**（2026-08-12）。本文件是当前唯一执行 SSOT；Phase 0 后再进 PRD（§7）。
目标消费者：`agenterm`、`agenterm-con`、`agenterm-cu`。
关联：[`ARCHITECTURE.md`](../ARCHITECTURE.md) §1.0、[`plan-v0.1.18.md`](../plan-v0.1.18.md) §1、
[`plan-ape-thin-shell-dynamic-packages.md`](../plan-ape-thin-shell-dynamic-packages.md)

---

## 1. 为什么做（和为什么不做）

| 动机 | 判定 |
|------|------|
| 省体积 | **待实测**。共享库可能消除三个消费者的重复机制，也会引入 feature 并集；每个库产物必须独立严格小于 1 MiB |
| 省构建时间 | **不成立**。ape 计划已测明靶心是 agenterm 那个 165 文件 monolith |
| **跨语言消费** | **成立**。cu 的后端参考是 Swift AX / Python UIA；wbox 等 embedding 同理 |
| **机制独立更新** | **成立**。改一个 ConPTY bug 不必重编六格三个二进制 |
| **边界机器可检** | **成立**。导出符号表比 prose 纪律强 |

立项主因是后三条；减少重复字节是需要 Phase 0/1 证明的收益，不是预支结论。

---

## 2. 两条硬规则

**规则一：函数体是 syscall 才能过 ABI，是纯算术就必须静态链接。**

| 进库 | 留静态 rlib |
|------|------------|
| `pty` `process*` `ipc` `filesystem*` `clipboard` `screenshot` `window*` `ime` `input_inject` `shared_memory` `runtime` `font`(已缓存) | **整个 `agenterm-ui-core`**；`numeric` `byte_search` `checksum` |

ui-core 是逐行热路径。PRD 24 记的 895→360 us 与零 host copy 全建立在直接 raster
进 retained XRGB buffer 上；每行穿一次 FFI 会把它吐回去。

**规则二：编译期 feature → 运行期能力查询。** dll 无法为每个消费者裁剪。
好在这与现有 `Available/Unsupported/Failed` 三态同构，只是把"这构建没编进"
并入同一通道。代价记账：con 的最小依赖图纪律从"链接期不含"退化为"运行期不调用"。
因此动态库本身必须 `<= 1,048,575 B`，迁移后的 con EXE 也必须独立满足同一上限。

---

## 3. 接口

`crate-type = ["cdylib"]`，C ABI。新增 `crates/agenterm-abi/` 薄导出壳（~2–3k 行），
`agenterm-platform` 那 47k 行原样不动。符号前缀 `agt_`，不含任何产品概念
（tab / workspace / Fleet / lease / instance）。

### 3.1 版本与错误

```c
uint32_t    agt_abi_version(void);   /* (major<<16)|minor；major 不符拒绝加载 */
const char* agt_build_id(void);      /* 语义版本 + 源 SHA */

typedef enum { AGT_OK=0, AGT_UNSUPPORTED=1, AGT_FAILED=2 } agt_status;

typedef struct {
  const char* operation;  /* 静态，永久有效 */
  const char* code;       /* 静态 */
  const char* message;    /* 线程局部，有效至本线程下次调用 */
} agt_error;
agt_status agt_last_error(agt_error* out);
```

现有 `PtyError` 已是 `Unsupported{operation,reason}` / `Failed{operation,code,message}`，
且 `operation`/`code` 本就是 `&'static str` —— 零分配即可暴露为稳定 C 字符串。
**不变量：这两态永不合并**（否则上层分不清"平台没有"和"这次没成"）。

### 3.2 能力协商

```c
typedef enum {
  AGT_CAP_PTY=1, AGT_CAP_PROCESS_SPAWN, AGT_CAP_PROCESS_OBSERVE,
  AGT_CAP_WINDOW_HOST, AGT_CAP_WINDOW_ENUMERATE, AGT_CAP_WINDOW_OP,
  AGT_CAP_SCREENSHOT, AGT_CAP_CLIPBOARD, AGT_CAP_IME, AGT_CAP_INPUT_INJECT,
  AGT_CAP_IPC, AGT_CAP_FONT_RASTER, AGT_CAP_FILESYSTEM_PUBLISH,
  AGT_CAP_SHARED_MEMORY, AGT_CAP_PARENT_CONSOLE
} agt_capability;

agt_status agt_capability_query(agt_capability);  /* 只返回 OK 或 UNSUPPORTED */
```

### 3.3 句柄与线程亲和

```c
typedef struct agt_pty*     agt_pty_t;
typedef struct agt_window*  agt_window_t;
typedef struct agt_process* agt_process_t;
typedef struct agt_frame*   agt_frame_t;
```

不透明，库拥有，显式 `_close`。**亲和性必须写进头文件**：

- `agt_window_t` / `agt_frame_t` / 字形 raster —— **创建线程专属**
  （Win32 消息泵与 `CreateCompatibleDC(NULL)` 的 HDC/HFONT 线程所有权）
- `agt_pty_t` / `agt_process_t` —— 跨线程安全

### 3.4 缓冲区：调用方分配，两段式

```c
/* cap 不足 → FAILED + code="buffer_too_small"，所需长度写进 out_len */
agt_status agt_pty_read(agt_pty_t, uint8_t* buf, size_t cap, size_t* out_len);
```

库**从不**把内存所有权交给调用方。彻底消灭"谁 free"。

### 3.5 事件：轮询，不回调

```c
typedef enum { AGT_EV_NONE=0, AGT_EV_GEOMETRY, AGT_EV_POINTER, AGT_EV_WHEEL,
               AGT_EV_KEY, AGT_EV_IME, AGT_EV_FOCUS, AGT_EV_EXPOSE,
               AGT_EV_CLOSE_REQUEST } agt_event_kind;

typedef struct { uint32_t kind; uint64_t generation; union { /* POD */ } data; } agt_event;

agt_status agt_window_poll_event(agt_window_t, agt_event* out, uint32_t timeout_ms);
```

回调穿 ABI 会把**重入**和 **unwind** 叠加，还把重入策略泄漏给调用方。
轮询把重入完全关在库内。

### 3.6 帧：借出裸指针，保住零拷贝

```c
typedef struct {
  uint32_t* pixels;  /* 借出，仅在 frame 存活期有效 */
  uint32_t  width, height, stride_px;
  uint64_t  generation;
  uint32_t  retention;   /* retained | transient */
} agt_frame_info;

agt_status agt_frame_begin  (agt_window_t, agt_frame_t* out, agt_frame_info*);
agt_status agt_frame_commit (agt_frame_t, const agt_pixel_rect* damage, size_t n);
agt_status agt_frame_abandon(agt_frame_t);
```

`n==0` → Full，`n>0` → bounded partial，`abandon` → None，与现有合同一一对应。
调用方用**静态链接的 ui-core** 直接写 `pixels`。
**不变量**：`commit`/`abandon` 后指针立即失效；debug 构建须毒化并在二次使用时
fail-closed，不靠文档约定。

### 3.7 PTY（其余模块同构）

```c
typedef struct { const char* program; const char* const* argv; size_t argc;
                 const char* cwd; const char* const* envp; size_t envc;
                 uint16_t cols, rows; } agt_pty_spawn;

agt_status agt_pty_open  (const agt_pty_spawn*, agt_pty_t* out);
agt_status agt_pty_write (agt_pty_t, const uint8_t*, size_t, size_t* written);
agt_status agt_pty_resize(agt_pty_t, uint16_t cols, uint16_t rows);
agt_status agt_pty_wait  (agt_pty_t, uint32_t timeout_ms, int32_t* exit_code);
void       agt_pty_close (agt_pty_t);
```

### 3.8 panic 围栏

每个导出函数 `catch_unwind` 兜底，转 `AGT_FAILED{code="panic"}`。
库必须以 `panic = "unwind"` 构建——与 con 的 `con-*` profile 同源理由。

---

## 4. 与 App Host ABI v1 的关系

```text
App guest (QJS .agp) ──Host ABI v1──▶ 产品层 ──libagenterm ABI──▶ OS
                        产品语义边界              机制边界
```

两条轴不同，**永不合并**。硬规则：**App guest 不得看到 `agt_*` 符号**，
否则 App 能绕过产品 authority 直接操作 OS。该规则进 `boundary_tests`。

---

## 5. 边界闸（三道）

1. **导出清单** —— `crates/agenterm-abi/exports.txt` 是唯一真相，生成 Windows `.def` /
   ELF version script / Mach-O `-exported_symbols_list`；实际符号集多一个少一个都红。
2. **头文件同步** —— `include/agenterm.h` 与实现比对，防漂移。
3. **产品名闸** —— 扩展现有测试：导出符号必须 `agt_` 前缀且不含产品概念词。

---

## 6. Phase 0：先出形态判决

只导出 `pty` + `process` + `window/frame` + `screenshot` 四组，
拿 `agenterm-con` 做**并行验证消费者**（保留现有静态版，另建 dylib 变体）。

| 判据 | 阈值 |
|------|------|
| 独立产物预算 | 每个平台 `libagenterm.{dll,so,dylib}` 和迁移后的 con EXE 各自 `<= 1,048,575 B`；不得用合并安装载荷均摊超标 |
| 共享收益 | 三个消费者迁移前后密封总字节实测；Phase 0 可暂时变大，但须给出盈亏平衡点，不得虚报节省 |
| 渲染性能 | 16-step resize journey 的 frame / full-candidate / dirty-pixel / native-present 四项，与静态版差异 **< 5%** |
| 行为等价 | 90 单测 + 21 GUI 黑盒 + 多标签控制旅程全绿；公开 CLI/JSON 合同字节不变 |

三条全过才进 Phase 1。

- **Phase 1**：`agenterm-cu` 首个真实消费者（跨语言理由的来源，无历史包袱）
- **Phase 2**：`agenterm-con` 迁入；EXE/库各自 sub-1-MiB，启动、PTY、渲染和清理不得回退
- **Phase 3**：`agenterm` 迁入并删除 ABI 已稳定承载的重复机制；server、脚本、mux、MCP
  和工作台产品语义留在产品层
- `agenterm-cc` 不在当前承诺消费者集合，后续只有明确需求与实测收益才评估

判据不过 → 本文件转 `archive/` 加 ⚠️ 横幅，写明否决理由与实测数字，不留残叶。

---

## 7. PRD 归属：规划已接受，Phase 0 后进入

现在没有 PRD 条目是有意的：共享机制方向和三个目标消费者已经接受，但 ABI、迁移和
产物尚未实现，不能虚报 shipped。Phase 0 判定具体动态库形态，而不是重新决定是否复用。

- 判据通过 → 开第三个 PRD 子树（编号自 32 起），拥有机制边界、ABI 稳定性承诺、
  能力协商模型与密封/SBOM 归属；`PRD_02_02` 登记 `.dll` 交付角色；`PRD_02_20` 记一条
  引用（**机制契约仍归 20，ABI 稳定性归新子树**——两回事）。
- 判据不过 → 归档。

在此之前任何 PRD 模块都不得把 libagenterm 写成已接受范围。

---

## 8. 非目标

- 不把 `agenterm-ui-core` 放进动态库。
- 不用 Rust ABI / `crate-type=["dylib"]`（绑编译器版本、无稳定性、要单独带 libstd）。
- 不做插件系统——本库是**导出**边界，不是**注入**边界。
- 不把"必然减小总体积"当预设；独立 sub-1-MiB 预算和重复字节实测仍是验收指标。
- ABI 里不出现任何产品概念；App guest 不得触达 `agt_*`。
- `.dll` 是第七个需密封与 SBOM 记账的产物，不是免检品。

---

*执行投影，非产品宪法。已接受规划、尚未实现。*
