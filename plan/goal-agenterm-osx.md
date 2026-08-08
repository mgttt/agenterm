# goal-agenterm-osx

状态：active  
角色：OSX 机跟进 agent（单写 Unix/macOS 泳道）  
编排拍板：2026-08-05 / 续写 2026-08-08  
关联：`plan/plan-v0.1.15.md`（§1 O 组 · §2.2.1 · §6 · §11）、`plan/ARCHITECTURE.md`（§6 禁令 · L2–L4）  
**脱敏**：禁止宿主绝对 home 路径与真实凭据；仓内**仓库相对路径**，家目录统一 **`~/...`**（跨 OS/ISA）。铁律：根目录 `Agents.md` → Document redaction。

---

## 0. 仓库与约束

- **CWD**：agenterm 仓库根（勿在其它 monorepo 树内改本仓）
- **先读**：`plan/ARCHITECTURE.md`、`plan/plan-v0.1.15.md` 上述章节
- **单写者**：`src/platform/adapters/unix/frontend/**`、`adapters/macos/**`；`src/frontend/*` 仅当语义真共享
- **禁**：Win IME 域；无证据宣称三端齐；worktree 开发；`git add -A`；默认 kill 用户 server
- **commit**：pathspec 精确提交
- **回执语言**：精简中文；技术决策已拍板的 **禁止** 再问董事长

---

## 1. 已拍板（直接执行）

| ID | 决策 |
|----|------|
| **O1b** | Unix 状态栏 **开工** IME 段（接 macOS ImeStatus；布局可跨平台） |
| **O-fix** | 认领并修 `prd_alignment_public_command_missing:delete-buffer`——补 buffer 族公开命令，**不是** flake |
| **G-P1** | 无 signed 时 **自动 unsigned-preview + 强制信任警告**（G1 可做） |
| **G-P2** | **不**默认 kill server；升级后须 **提示版本滞后**（keep-server 会挂旧进程） |
| **P-P1** | v0.1.15 **text-only**；T1 非法 UTF-8 默认 lossy **不做**；T2 类型感知粘贴 **→ v0.2.x** |

---

## 2. 本机已验证事实（勿空转重测当主业）

- `agenterm-con` 在本机 macOS **可顺利打开**
- `cargo test --bin agenterm-con` → **43 pass**
- `cargo test --test agenterm_con_blackbox` → 约 **3 pass / 9 fail / 2 ignore**  
  - 失败主因：测试硬编码 **`cmd.exe`**（Win-centric），**不是** con 打不开
- 亲测：`./target/debug/agenterm-con --no-activate --cols 80 --rows 24 --emit-snapshot … --script …`  
  - `child_alive=true`，屏上可见 `DEF_OK` / `CON_OK`，title `agenterm-con — SF Mono`
  - 默认 shell 可能是 bash+py38 提示，PTY 仍正常
- **O6** Shift+选区复制：定因与止血见 plan §11.8 / `fb573f9` 一带  
  - 接手先 `git log --oneline -20` + 真机 `pbpaste` 复核；已关叶勿重开除非回归红

---

## 3. 可执行工作树（优先序）

### 3.1 O-fix（红灯 · 优先）

- [ ] 复现 `prd_alignment_public_command_missing:delete-buffer`（命令以 plan/树内为准）
- [x] 修：buffer 族（含 `delete-buffer`）进入 **public command** 对齐
- [ ] 验收：亲测同命令绿；回执写清 PRD/registry/catalog 路径

### 3.2 O1b（Unix 状态栏 IME）

- [x] 读 `crates/agenterm-platform/src/adapters/macos/ime.rs`（真实现已接线，未回退 stub）
- [x] **shared-first**：状态段布局进入 `src/ui_geometry.rs`；Unix adapter 只 present
- [x] 对照共享 `ImeStatus::label()` 文案，未复制 Win 宿主逻辑
- [ ] 验收：真机切 ABC↔中文状态栏可见变化；相关 unit 绿；`./check.sh --quick` 能过则过

### 3.3 G1（可选 · G-P1 已解锁）

- [ ] install：无 signed → 自动 unsigned-preview + **强制**信任警告
- [ ] 勿静默；G2/G7a 已落地则只复核

### 3.4 agenterm-con 跨平台测试债

- [x] blackbox 去掉硬编码 `cmd.exe` 启动参数：按平台选 `$SHELL`/`/bin/sh` vs `cmd.exe`
- [ ] 目标：本机 blackbox 从假红 → 真绿或有理由 ignore
- [ ] **不**扩 con 产品范围（无 tab/server 是设计）

---

## 4. 结构债 / 抽象与复用

债务钩子：`ARCHITECTURE.md` L2/L3/L4。  
**大拆 HOLD**，除非有明确小 PR 边界 + 测绿 + 不扩产品语义。

| 优先级 | 问题 | 方向 | 禁踩 |
|--------|------|------|------|
| P0 | Win `remote_frontend` / Unix `frontend/mod` 双主机巨石；`ui-action` 大 match 双写 | 新交互 shared-first（`src/frontend/*` + `ui_action_catalog`）；表驱动 action；host 只 present/wake/IME | 一端偷偷双写；整文件大搬家无测 |
| P1 | selection/focus/wheel 仍有宿主分叉 | 新逻辑优先已共享模块（如 `interaction.rs`）；宿主薄适配 | 复制整段 host 逻辑 |
| P1 | 粘贴只 `get_text` + normalize 掐 control | v0.1.15 只修诊断/错误可见；类型感知归 v0.2 | 本版上 HTML/image MIME |
| P2 | `agenterm-con` 与主产品 VT/选区/键位部分重复 | 抽纯函数（选区文本、key→bytes）到 platform/shared | 把 Fleet/server 塞进 con |
| P2 | SSOT 机读不全 | 可选扩 `boundary_tests` bins/目录闸；S2/S3 大方案 HOLD | 为对齐写第二现实文档 |
| P3 | install/升级体验 | G-P1/G-P2 行为；version lag 提示 | 默认 kill server |

微重构切片原则：

1. 先相关 `boundary_tests` + lib 测绿  
2. 优先 **≤1 个巨石文件的垂直切片**（如单一 action 表驱动化）  
3. 每切片可独立 `cargo test`；无证据不宣称完成  

---

## 5. 明确不做

- 不默认 `git push`
- 不扩 L-NET / ipfs / Fleet 全量
- 不改 `.github/workflows` cache 键（除非修自己引入的红）
- 不把 blackbox Win 假红当 OSX 产品 P0
- 已拍板项回执禁止「请用户定」——做不动报诊断 + 阻塞原因

---

## 6. 开工命令

```bash
# 在 agenterm 仓库根执行
git status -sb && git log --oneline -15

cargo test --bin agenterm-con
cargo test --test agenterm_con_blackbox
# 可选：
./check.sh --quick

# O1b / O-fix 相关单测以 plan 与源码为准；禁止降绿线换假绿
```

---

## 7. 验收回执模板（必须）

```
已达成:
- <命令> → <绿/红原文摘要>
- 改动 pathspec: …

可改进:（≥3，含文件路径 + 可观测信号）
1. …
2. …
3. …

未做/阻塞:
- …
```

---

## 8. 与 plan-v0.1.15 的关系

- **本文件** = OSX 机可转发的 goal / 派工 SSOT 切片（执行序 + 拍板 + 验收集）
- **plan-v0.1.15.md** = 全版素材与收敛树；O/G/P 细节与 §11 定因仍以彼为准
- 冲突时：拍板表以本文件 §1 与 plan §6 双写一致为准；细节叙事回 plan §11

---

## 9. 北极星（本 goal）

1. 灭 **O-fix** 红灯  
2. 落地 **O1b** 状态栏 IME  
3. 顺手修 **con blackbox** 跨平台假红  
4. 抽象只做 **有测的小切片**

每一步以可复现绿线证明存在；不报虚绩。
```
