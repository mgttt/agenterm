# 作战小队邮箱（SSOT）

协议：[inter-agent-comms.md](inter-agent-comms.md)  
登记表：[session-registry.md](session-registry.md)

最后由**主控2**刷新：2026-08-05

## 共享事实

| 键 | 值 |
|----|-----|
| 产品版本 | **0.1.14**（`Cargo.toml`）；v0.1.15 计划另案 |
| 当前主线任务 | **Built-in skins v1**（classic/fancy × day/night） |
| 产品契约 | `prd/PRD_02_06_human_workspace.md` § Built-in skins (v1) |
| 执行计划 | `plan/plan-skins-v1.md` |
<<<<<<< HEAD
| `origin/main` | tip 含设计；分身4 `cursor/skins-eng-phase2a` @ `c9641f1` **审阅打回**（theme-smoke/Win layout/title） |
=======
| `origin/main` | tip 含设计冻结 @ 09b275a+；分身4 Phase 2B 审阅阻断已修，待再审 |
>>>>>>> origin/cursor/skins-eng-phase2a
| 云环境 | Personal `mgttt/agenterm`；x86_64 Linux + DISPLAY=:1 |
| SkinHub / 外置皮肤包 | **不做**（M14）；本任务仅内置四预设 |

## 主控指令（未消化则分身不得另起炉灶）

### → 分身3（皮肤设计）
1. `git pull --ff-only origin main`；读 PRD_02_06 皮肤节 + `plan/plan-skins-v1.md`。
2. **仅**写 `assets/skins/**`（manifest、四套 ThemePalette hex、品牌/标题模板、icon 方向与占位图、Settings 描述文案）。
3. classic-day/night ≈ 今日 Light/Dark 精神；fancy 更有品牌感但仍工业可信；WCAG AA；ANSI 可读。
4. **禁止**改 `src/**`、`scripts/rhai/**`、`prd/**`。
5. 小步 commit；可开 `cursor/skins-design-*`；合入由主控2 审。回报 SHA + 文件清单。

### → 分身4（皮肤工程）
1. `git pull --ff-only origin main`；读同上契约与计划。
2. Phase 2A：`AppearancePreset` / Skin×Luminance；classic 映射现有 DARK/LIGHT；fancy 可先 alias 但 **id 必须四分**；迁移 `color_theme`；locale + settings UI + `theme-smoke` + snapshot。
3. Phase 2B：**只读消费** `assets/skins/**`（已合 main）；可生成/补 `fancy/icon.ico|.icns`，勿另起一套色板。
4. 勿抢 `src/lib.rs` / `Cargo.toml` / `PRD.md`（需改时写请示）。
5. 小步 commit；合入由主控2 审。Phase 2B 等设计合 main 后再吃 palette/icon。

## 请示队列

（空 — 皮肤开工）

## 席位状态

### 主控2 · 2026-08-05
- 状态: RUNNING — 编排皮肤 v1
- 分支: `main`
<<<<<<< HEAD
- 下一步: 等分身4 修复 theme-smoke/Win layout/title 后复审
=======
- 下一步: 再审 `cursor/skins-eng-phase2a` Phase 2B 阻断修复
>>>>>>> origin/cursor/skins-eng-phase2a
- 阻塞: 无
- 已合: 分身3 Phase 1（`cursor/skins-design-v1` → main）

### 分身3 · 2026-08-05
- 状态: IDLE — Phase 1 已合 main @ 09b275a
- bcId: `bc-f2d5f6f1-41c0-44b1-bd74-1b80f036013b`
- URL: https://cursor.com/agents/bc-f2d5f6f1-41c0-44b1-bd74-1b80f036013b
- 分支: —（设计叶已删）
- 证据: `assets/skins/**`
- 下一步: 待命；fancy 终稿 icon 再开 `cursor/skins-design-icon`（仅 `assets/skins/fancy/icon.*` + `icon-direction.md`）
- 阻塞: 无

### 分身4 · 2026-08-05
<<<<<<< HEAD
- 状态: ACTIVE — Phase 2B 审阅打回，修阻断中
- bcId: `bc-c3e01145-b870-4e74-b18c-f3aea06ea800`
- URL: https://cursor.com/agents/bc-c3e01145-b870-4e74-b18c-f3aea06ea800
- 分支: `cursor/skins-eng-phase2a` tip `c9641f1`（**未合**）
- 打回: theme-smoke owned_children；迁移 close-window；落盘断言；Win Inherit 重叠；Win 创建标题分裂
- 下一步: 修 Critical/Major 后回报新 tip
- 阻塞: 主控2 不合 main 直至修复
=======
- 状态: IDLE — 审阅阻断已修，待主控2 再审
- bcId: `bc-c3e01145-b870-4e74-b18c-f3aea06ea800`
- URL: https://cursor.com/agents/bc-c3e01145-b870-4e74-b18c-f3aea06ea800
- 分支: `cursor/skins-eng-phase2a` @ `f3cd0f4`
- 修复: smoke owned_children + migration close-window；load 落盘 appearance_preset；Win inherit 几何 + 创建标题；startup-smoke 标题对齐
- 延后: Apply 后 Linux icon 不刷新；Win exe embed 未切 fancy.ico；metrics 未进 render（见 plan）
- 证据: `cargo test --lib` 620 passed
- 下一步: 待命
- 阻塞: 无
>>>>>>> origin/cursor/skins-eng-phase2a
