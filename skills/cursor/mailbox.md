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
| `origin/main` | tip 含分身3 设计冻结（`assets/skins/**`）；工程可开 Phase 2B |
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
- 下一步: 审合分身4 Phase 2A；授权 Phase 2B 消费 `assets/skins/**`
- 阻塞: 无
- 已合: 分身3 Phase 1（`cursor/skins-design-v1` → main）

### 分身3 · 2026-08-05
- 状态: DONE — Phase 1 已合 main
- bcId: `bc-f2d5f6f1-41c0-44b1-bd74-1b80f036013b`
- URL: https://cursor.com/agents/bc-f2d5f6f1-41c0-44b1-bd74-1b80f036013b
- 分支: `cursor/skins-design-v1`（已 ff 合 main，可删）
- 证据: `assets/skins/**` @ 合入 tip
- 下一步: 待命；品牌终稿 icon 若需再开叶
- 阻塞: 无

### 分身4 · 2026-08-05
- 状态: ACTIVE
- bcId: `bc-c3e01145-b870-4e74-b18c-f3aea06ea800`
- URL: https://cursor.com/agents/bc-c3e01145-b870-4e74-b18c-f3aea06ea800
- 分支: `cursor/skins-eng-*`（自建）
- 下一步: 完成 Phase 2A 后立刻 Phase 2B（消费 main 上 `assets/skins/**`）
- 阻塞: 无（设计已合 main）
