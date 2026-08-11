# 抽象与复用专项 review：agenterm-con 与主 crate 的重复代码地图

| 字段 | 值 |
|------|-----|
| **主题** | 提升抽象与复用——识别 con 与主 crate 之间的重复代码，给出提取优先序 |
| 日期 | 2026-08-11 |
| 状态 | 只读分析 + 提案（未排期，非任务单） |
| 作者 | Grok（xai）+ GLM 5.2 协作产出 |
| 数据来源 | `src/bin/agenterm-con*.rs`、`src/bin/agenterm-con/*.rs`、`src/ui_geometry.rs`、`crates/agenterm-ui-core/` 实际代码扫描 |
| 关联 | `plan/glm-advance-ideas.md`（三层温度架构哲学）、`plan/design-frontend-shared-core.md`（双前端巨石五大提取候选）、`plan/ARCHITECTURE.md`（L2 债务） |
| 范围声明 | **只读 + 分析提案**；不修改任何 `.rs` 文件 |

---

## 0. 一句话

`agenterm-ui-core` crate 已经存在，con 已经在用它——但**主 crate 只用了它的 scrollbar 几何，
其余 UI 原语（布局常量、Rect 类型、选区逻辑、VT 回调、PTY 线程模式）仍然两套各写一份**。
本文扫描出具体的重复点，按收益排序给出提取路线。

---

## 1. 现状：共享层的真实状态

### 1.1 `agenterm-ui-core` 已有什么

| 模块 | 内容 | 被谁用 |
|------|------|--------|
| `pixel.rs` | XRGB 打包、像素操作 | con ✅、主 crate ✅ |
| `glyph_cache.rs` | `GlyphCache` / `GlyphCacheKey` | con ✅、主 crate ✅ |
| `damage.rs` | `DirtyRegion` / `DirtyRows` / `PixelRect` | con ✅、主 crate ✅ |
| `retained_frame.rs` | `RetainedXrgbFrame` | con ✅、主 crate ✅ |
| `tree.rs` | `compute_tree_depths` | con ✅、主 crate ✅ |
| `lib.rs` 顶层 | `Rect`、`ScrollbarGeometry`、`terminal_scrollbar_geometry()` | con ✅、主 crate ✅（经适配层） |

**核心发现**：像素/字形/dirty/tree 这些渲染原语**已经共享了**。这不是一个空壳 crate——
它已经承载了 con 和主 crate 共同的基础设施。

### 1.2 con 的模块字节量（背景数据）

| 文件 | 字节 | 行数 | 占比 |
|------|------|------|------|
| `agenterm-con.rs`（主入口） | 230,628 | 5,827 | **64.5%** |
| `agent_interface.rs` | 41,761 | 1,072 | 11.7% |
| `control.rs` | 31,956 | 977 | 8.9% |
| `json.rs` | 19,046 | 593 | 5.3% |
| `ui.rs` | 12,296 | 398 | 3.4% |
| `bitmap_glyphs.in.rs` | 5,093 | 99 | 1.4% |
| `font.rs` | 4,694 | 155 | 1.3% |
| `palette.rs` | 4,526 | 143 | 1.3% |
| `workspace.rs` | 3,806 | 133 | 1.1% |
| `composer.rs` | 3,512 | 118 | 1.0% |
| **合计** | **357,318 (349 KB)** | **9,515** | **100%** |

主入口 `agenterm-con.rs` 是 230 KB 巨石——con 的几乎全部逻辑住在里面。

---

## 2. 重复点清单（按提取收益排序）

### 2.1 🔴 P0：布局常量 + Rect 类型——两套系统已经漂移

#### 事实

| 概念 | con (`ui.rs`) | 主 crate (`ui_geometry.rs`) | 状态 |
|------|--------------|---------------------------|------|
| Rect 类型 | `Rect { x, y, width, height }` (u32) | `PixelRect { left, top, right, bottom }` (i32) | **类型不同** |
| Composer 高度 | `COMPOSER_HEIGHT_DIP: 96.0` (f64 DIP) | `COMPOSER_HEIGHT: 104` (i32 px) | **值不同（96 vs 104）** |
| Sidebar 默认宽度 | `SIDEBAR_WIDTH_DIP: 224.0` (f64 DIP) | `TABS_DEFAULT_WIDTH: 250` (i32 px) | **值不同（224 vs 250）** |
| Sidebar 最小宽度 | `SIDEBAR_MIN_WIDTH_DIP: 180.0` | `TABS_MIN_WIDTH: 180` | 值相同，单位不同 |
| Sidebar 最大宽度 | `SIDEBAR_MAX_WIDTH_DIP: 480.0` | `TABS_MAX_WIDTH: 480` | 值相同，单位不同 |
| Terminal 最小宽度 | `TERMINAL_MIN_WIDTH_DIP: 320.0` | `TERMINAL_MIN_WIDTH: 320` | 值相同，单位不同 |
| Tree 行高 | `TREE_ROW_HEIGHT_DIP: 30.0` | `TAB_HEIGHT: 36` | **值不同（30 vs 36）** |
| 滚动条宽度 | `TERMINAL_SCROLLBAR_WIDTH_DIP: 12.0` | `TERMINAL_SCROLLBAR_WIDTH: 12` | 值相同，单位不同 |

#### 根因

con 是 **DIP（device-independent pixel）+ f64 + scale 参数**体系，主 crate 是
**pixel + i32** 体系。两套体系对应不同 DPI 策略，但语义相同的概念产生了名字和值的分叉。

#### 提取方案

在 `agenterm-ui-core` 定义一组 `LayoutConstants`（逻辑单位 DIP），两端各自应用 scale：

```rust
// crates/agenterm-ui-core/src/layout.rs
pub struct LayoutConstants {
    pub composer_height_dip: f64,
    pub sidebar_min_width_dip: f64,
    pub sidebar_default_width_dip: f64,
    pub sidebar_max_width_dip: f64,
    pub terminal_min_width_dip: f64,
    pub tree_row_height_dip: f64,
    pub terminal_scrollbar_width_dip: f64,
}

pub const DEFAULT_LAYOUT: LayoutConstants = LayoutConstants {
    composer_height_dip: 96.0,   // 统一到 con 的值（或重新校准）
    sidebar_min_width_dip: 180.0,
    sidebar_default_width_dip: 224.0,
    sidebar_max_width_dip: 480.0,
    terminal_min_width_dip: 320.0,
    tree_row_height_dip: 30.0,
    terminal_scrollbar_width_dip: 12.0,
};
```

两端各自 `dip_to_pixel(constant, scale)`。**值差异（96 vs 104、224 vs 250、30 vs 36）必须在
提取时裁决——这本身就是已经发生的隐性漂移。**

#### 收益

- 消除隐性漂移（con 和主 crate 的布局比例已经不同）
- 布局变更一处生效
- 为跨平台 UI parity 提供单一真相

---

### 2.2 🔴 P1：文本选区逻辑——con 从零重写了一遍

#### 事实

con 的 `agenterm-con.rs` 有：

```rust
struct TerminalPoint { row, col }           // 行 146
fn selection_should_auto_copy(...)           // 行 151
fn selection_text(screen: &vt100::Screen, a, b)  // 行 168
impl TerminalPoint { normalize, compare }    // 行 155
```

主 crate 的 `frontend/selection.rs` + `frontend/text_selection.rs` 有同类逻辑：
cell 位置比较、wide-cell 跳过、CJK 处理、CRLF、multiline。`design-frontend-shared-core.md`
§1 #2 也点名了这个重复（"双/三击判定谓词逐字节相同"）。

#### 提取方案

新建 `crates/agenterm-ui-core/src/selection.rs`（或独立 `agenterm-terminal-core` crate）：

```rust
pub struct TerminalPoint { pub row: usize, pub col: usize }

pub fn normalize_selection(a: TerminalPoint, b: TerminalPoint)
    -> (TerminalPoint, TerminalPoint)

pub fn selection_text(screen: &vt100::Screen, start: TerminalPoint, end: TerminalPoint)
    -> String

pub fn word_boundary(screen: &vt100::Screen, point: TerminalPoint)
    -> (TerminalPoint, TerminalPoint)
```

con 和主 crate 都消费这些函数。这也消化 `design-frontend-shared-core.md` §1 #2（选区生命周期）。

#### 收益

- 消除 con 的 `selection_text`（~70 行）和主 crate 的对应代码
- 统一 wide-cell / CJK / CRLF 处理（当前可能已经有行为差异）
- 消化一个已挂账的 L2 提取候选

---

### 2.3 🟡 P2：VT 回调 + PTY 读取线程模式

#### 事实

con 的 `ConCallbacks`（行 68-144，实现 `vt100::Callbacks`）处理：
- DA1 查询应答（`CSI c`）
- CPR 查询应答（`CSI 6n`）
- OSC title 捕获
- unhandled_csi 回退（行 80-144 的注释详细记录了"DA1 不应答导致 TUI 挂死"的根因）

主 crate 的 `terminal_runtime.rs` / `terminal_lifecycle.rs` 有同类 PTY 读取线程和 VT 处理。

con 的注释明确说它用了"和产品终端同一个 hardened vt100 parser"——但**回调策略是各写一份**。

#### 提取方案

定义 VT 回调策略为可配置 struct（不是 trait——避免过度抽象）：

```rust
// crates/agenterm-ui-core/src/vt_strategy.rs
pub struct VtCallbackStrategy {
    pub answer_da1: bool,      // DA1 查询应答
    pub answer_cpr: bool,      // CPR 位置查询应答
    pub capture_osc_title: bool,
    pub pending_writes: VecDeque<Vec<u8>>,
}

impl VtCallbackStrategy {
    pub fn unhandled_csi(&mut self, screen, intermediate, params, final_byte)
        -> Option<Vec<u8>>  // 返回要写回 PTY 的应答
}
```

两端各自构造策略实例，共享应答逻辑。

#### 收益

- DA1/CPR 应答逻辑一处维护（con 花了大篇幅调试这个 bug，主 crate 也有同类风险）
- PTY 读取线程（独立线程 + bounded pipe + resize 防抖）的模式可以共享

---

### 2.4 🟡 P3：cell 渲染（paint_cells）

#### 事实

con 的 `paint_cells()` / `paint_cells_at()` / `paint_chrome_text()`（行 4625-4940）是
cell → pixel 渲染。主 crate 的 Win GDI paint（`remote_frontend.rs`）和 Unix `render.rs`
各有一套。

#### 提取方案

远期——定义 `CellRenderer` trait：

```rust
pub trait CellRenderer {
    fn paint_cell(&mut self, rect: PixelRect, cell: &vt100::Cell, glyph: &Glyph);
    fn paint_chrome_text(&mut self, rect: PixelRect, text: &str, color: Rgb);
}
```

两端只提供 pixel blit 后端（Win GDI / Unix softbuffer），cell 遍历和 dirty 计算共享。

#### 收益

- 这是 `design-frontend-shared-core.md` 说的"渲染层才是真正平台特有的"——但 cell 遍历不是
  平台特有的，可以共享
- 远期收益，当前优先级低

---

### 2.5 🟢 P4：scrollbar 几何（已基本统一，剩余适配层）

#### 事实

主 crate 的 `ui_geometry.rs`（行 458-481）已经通过适配层调用
`agenterm_ui_core::terminal_scrollbar_geometry()`：

```rust
// 主 crate 已经在做：
pub(crate) fn terminal_scrollbar_geometry(terminal: PixelRect, ...) -> TerminalScrollbarGeometry {
    let geometry = agenterm_ui_core::terminal_scrollbar_geometry(
        agenterm_ui_core::Rect { left: terminal.left, top: terminal.top, ... },
        ...
    );
    TerminalScrollbarGeometry {
        track: PixelRect::from(geometry.track),
        thumb: PixelRect::from(geometry.thumb),
    }
}
```

**scrollbar 几何已经统一了**——主 crate 只保留了一层 `PixelRect ↔ ui_core::Rect` 类型适配。

#### 剩余动作

如果 Rect 类型统一了（P0），这层适配可以消失。

---

## 3. 提取路线图

```
agenterm-ui-core (现状)          agenterm-ui-core (目标)
┌────────────────────┐           ┌────────────────────────┐
│ Rect               │           │ Rect                   │  ← 唯一 Rect
│ ScrollbarGeometry   │           │ ScrollbarGeometry      │
│ terminal_scrollbar │           │ terminal_scrollbar     │
│ GlyphCache         │           │ GlyphCache             │
│ DirtyRegion        │           │ DirtyRegion            │
│ RetainedXrgbFrame  │           │ RetainedXrgbFrame      │
│ tree_depths        │           │ tree_depths            │
│                    │           │                        │
│                    │           │ + LayoutConstants  (P0)│  ← 布局常量统一
│                    │           │ + selection_text   (P1)│  ← 选区共享
│                    │           │ + VtCallbackStrategy(P2)│ ← VT 应答共享
│                    │           │ + CellRenderer trait(P3)│ ← 远期
└────────────────────┘           └────────────────────────┘
       ↑                                ↑       ↑
       │                                │       │
  con (已用)                        con    主 crate
                                    都用   都用
```

| 阶段 | 做什么 | 消化哪个债务 | 预估改动 |
|------|--------|------------|---------|
| **P0** | `LayoutConstants` → `ui-core`；裁决值差异（96 vs 104、224 vs 250、30 vs 36） | 隐性布局漂移 | `ui-core` 加 1 文件；con `ui.rs` 和主 crate `ui_geometry.rs` 改常量来源 |
| **P1** | `selection.rs` → `ui-core`（或新 crate）；提取 `TerminalPoint` + `selection_text` + word boundary | L2 `design-frontend-shared-core.md` §1 #2 | `ui-core` 加 1 文件；con 删 ~70 行；主 crate frontend/selection.rs 改 import |
| **P2** | `VtCallbackStrategy` → `ui-core`；共享 DA1/CPR/OSC 应答 | con 调试的 bug 在主 crate 也有同类风险 | `ui-core` 加 1 文件；con ConCallbacks 简化；主 crate terminal_runtime 改 |
| **P3** | `CellRenderer` trait（远期） | L2 渲染层共享 | 大改，暂不排期 |

---

## 4. 关键约束

1. **值差异裁决必须有产品决策**。con 的 `COMPOSER_HEIGHT_DIP: 96` 和主 crate 的
   `COMPOSER_HEIGHT: 104` 哪个对？还是它们故意不同（con 是轻量控制台，主 crate 是完整产品）？
   如果故意不同，`LayoutConstants` 需要支持 per-app override。

2. **不要为了复用而复用**。`design-binary-size-and-reuse.md` 明确记录了"不是所有重复都值得
   一个抽象"。P0 和 P1 有明确的漂移证据（值不同 / 逐字节相同），值得提取。P2 和 P3 的提取
   成本更高，应等 P0/P1 验证收益后再推进。

3. **保持 `agenterm-ui-core` 的纯净**。它目前不含任何产品名 / Win32 / 依赖——只有数学和
   类型。提取新内容时必须保持这个约束（`boundary_tests` 风格）。

4. **con 的巨石 `agenterm-con.rs`（230 KB / 5827 行）是独立问题**。本文关注的是 con 与
   主 crate 之间的复用；con 内部的模块拆分是另一个维度的工作（codex 的 frozen 层建设）。

---

## 5. 与 glm-advance-ideas.md 的关系

本文是 `glm-advance-ideas.md` §2（三层温度架构）中 **frozen 层共享原语**的具体执行地图。

- `agenterm-ui-core` 就是 frozen 层的"渲染/几何原语 crate"
- P0-P3 的提取过程 = 把散落在 hot/cold 层的 frozen 级原语**归位**到独立 crate
- 每提取一个原语，con 和主 crate 的 frozen 层就同时增厚一点
- 这与 codex 在 con 的汇编/FFI 深度工作互补——codex 往下挖（FFI），本文往横收（共享原语）
