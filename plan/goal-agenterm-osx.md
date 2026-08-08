# goal-agenterm-osx

状态：active（O1b 与 con 黑盒闭环已达成，待 G1 新 Candidate 真机安装回执）
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

## 2. 当前执行回执（2026-08-08）

证据口径：本节的“仓库静态证据”来自当前源码；命令结果若标为“既有回执”，
仅沿用本文件已记录的真机结果。本次回执面整理未重跑构建、测试或 GUI 验收，
因此不得据此新增 PASS。

### 2.1 已达成

| ID | 已达成事实 | 可提交证据 |
|----|------------|------------|
| **O-fix** | `delete-buffer` / `deleteb` 已进入公开命令、mux 能力与控制分发 | `src/commands.rs`、`src/control_dispatch.rs`、`src/client/mod.rs`、`scripts/rh/cli-smoke.rh` 均有对应入口；既有回执为 PRD alignment 绿且 `list-commands` 可见 `delete-buffer (deleteb)` |
| **O1b-实现** | macOS 输入源状态已有真实适配，状态栏布局遵循 shared-first，Unix 宿主只负责呈现 | `crates/agenterm-platform/src/adapters/macos/ime.rs` 读取当前输入源并产出 `ImeStatus`；`src/ui_geometry.rs` 与 `src/platform/adapters/unix/frontend/**` 承接共享布局和宿主呈现 |
| **O1b-快照补齐** | shared/synthetic 快照加入 IME 字段 | `src/ui_snapshot.rs` 新增 `ime_status_snapshot_json`；`src/platform/adapters/unix/frontend/mod.rs` 使用共享序列化，产出 `layout.status_bar.ime` |
| **O1b-真机** | **PASS（可验证）**：状态栏 IME 已全量入快照，三态显示完整，且无截断 | 证据：`~/.local/share/agenterm/evidence/o1b-ime-20260808T125858Z`；`IME` 宽度为 220×26，`focus.window_focused=true`，`event_position` 完整；`result.txt` 记录 `abc/zh/abc-return` 各态均 `PASS` |
| **G1-实现** | unsigned-preview 分发、provenance 校验及 keep-server 行为已有代码/任务证据 | `install.sh` 使用 `download_with_http_status` 区分 signed/404/410，写入 `distribution`/`channel`/`variant`，并做 `provenance` + `sha256` 校验；`build-releases-index.rh` 目前作为发布索引证据，安装器当前不消费该索引 |
| **G1-实现补齐** | installed record 新增 distribution 字段 | `install.sh` 已写入 `distribution`（`stable`/`preview`/`local`），`tests/install_local_macos.rs` 校验本地链路记录与日志 |
| **con-shell** | blackbox 已按平台选择 shell，不再把 `cmd.exe` 作为 Unix/macOS 启动前提 | `tests/agenterm_con_blackbox.rs`：Windows 使用 `cmd.exe`，Unix/macOS 使用有效 `$SHELL`，否则回退 `/bin/sh` |
| **con-基础** | `agenterm-con` 的 macOS 打开、PTY 存活与基础单测已有历史真机证据 | 既有回执：`cargo test --bin agenterm-con` 为 43 pass；脚本快照曾记录 `child_alive=true`，画面含 `DEF_OK` / `CON_OK` |
| **O6** | Shift+选区复制已有定因与止血记录 | 证据归属 `plan/plan-v0.1.15.md` §11.8；除非回归复现，不在本 goal 重开实现 |
| **con-blackbox** | **PASS（可验证）**：`nonexistent_program_via_dash_e_exits_cleanly_instead_of_hanging -- --exact` 已全绿 | 证据：`cargo test --test agenterm_con_blackbox nonexistent_program_via_dash_e_exits_cleanly_instead_of_hanging -- --exact --nocapture`；`1 passed; 0 failed`，退出码 `0` |

### 2.2 未完成

| ID | 未完成项 | 完成判据 |
|----|----------|----------|
| **G1-真机** | 发布侧尚无满足重验条件的 `>=v0.1.15` 资产，闭环未完成 | 完成判据直接引用 §12.2 |

### 2.2.2 最近回执（执行位点：`2026-08-08`）

- O1b（IME）：`~/.local/share/agenterm/evidence/o1b-ime-20260808T125858Z`
  - **PASS（可验证）**：`abc / zh / abc-return` 三套 `snapshot.json`、`png`、`input-source.txt` 均生成；`layout.status_bar.ime.label/bounds` 均存在且有效
  - `abc` / `abc-return` 均为 `IME: off`，`zh` 为 `IME: 微信输入法 · native`
  - 三态 `bounds` 均为 `220×26`，`focus.window_focused=true`，`event_position` 正常
  - 960×600 逻辑尺寸下中文标签完整显示，无视觉截断
  - 代码层补齐 `ime` 投影链路可见：`src/ui_snapshot.rs`、`src/platform/adapters/unix/frontend/mod.rs`；共享状态栏几何调整在 `src/ui_geometry.rs` 完成并通过 `cargo test --lib ui_geometry`
- G1：`~/.local/share/agenterm/evidence/g1-install-20260808T120109Z`
  - signed HTTP 404 后自动 fallback 到 unsigned preview，并出现 trust warning
  - SHA-256/provenance 校验通过，HTTP 500 场景 fail-closed
  - 最新补充：`~/.local/share/agenterm/evidence/g1-install-20260808T124504Z` 与 `...T123730Z` 说明 remote `latest` 与资产契约仍不满足重放条件；仍未形成 installed 闭环
- 最近根因仍在发布侧：`latest` 未指向 `v0.1.15+`；preview payload 使用旧 `agenterm-rhai` 名，缺 `agenterm-rh`
  - 待 Candidate Promotion 后可见于 `/releases/latest` 的正式发布 `tag >= v0.1.15` 时重试；命令与判据见 G1 子项
- con-blackbox：
  - 运行命令：`cargo test --test agenterm_con_blackbox nonexistent_program_via_dash_e_exits_cleanly_instead_of_hanging -- --exact --nocapture`
  - `1 passed; 0 failed`，退出码 `0`，结论 PASS
- 集成门：当前源码状态没有本轮新鲜的 macOS `./check.sh --quick` 回执；需补充 PASS 或保留失败原文、复现命令和与本 goal 的相关性判断

**新增复核（并行子代理）**：
- G1 release 前置判断复核：`/releases/latest` 仍为 `v0.1.14`，未到 `>=v0.1.15`；`macos-aarch64` 仅见 unsigned-preview，且 preview 内部无 `agenterm-rh`；当前安装 fallback 与闭环不满足可重验条件。
- 下次有效重验触发条件：Candidate Promotion 后可见于 `/releases/latest` 的正式发布 `tag >= v0.1.15`；若 signed 为 200，走 signed 安装验收；仅当 signed 为 404/410 时，才要求 unsigned-preview 资产齐备（含 `agenterm`、`agenterm-cli`、`agenterm-rh`）且 `sha256 + provenance` 可下载，并走 §11 fallback 专项验收。
- 建议观察时刻锚点：`2026-08-09 10:00`（按子代理约定）。

### 2.3 阻塞

| ID | 阻塞边界 | 安全结果 |
|----|----------|----------|
| **G1-分发环境** | 真机安装闭环依赖 Candidate Promotion 后可见于 `/releases/latest` 的正式发布 `tag >= v0.1.15`，以及与所选分支匹配的完整资产 | 2026-08-08 最新回执：`latest` 与资产契约不满足新版本重放（`latest` 停留在 `v0.1.14`、`agenterm-rh` payload 问题）；资产、伴随校验文件或签名状态不满足对应分支时记录原始结果并停止，不静默降级，不默认停止用户 server |
| **发布侧状态** | GitHub `latest` 仍为 `v0.1.14`，尚无可触发任一 G1 分支的 `>=v0.1.15` 已发布资产 | API 现状：`tag_name=v0.1.14`, `published_at=2026-08-04T19:37:48Z`；仅见旧版 `agenterm-0.1.14-macos-aarch64-unsigned-preview.zip` 与相关 `*.sha256/.provenance.json`，不能用于本轮 `>=v0.1.15` 重验 |

当前未确认新增编译阻塞；`./check.sh --quick` 与 `./build.sh` 在本轮仍有既有 rh_fail 侧问题（仅作为外部待解），不与 O1b 证据判定直接冲突。

---

## 3. 下一步人工真机验收

优先闭环 **G1**。从仓库根启动当前 macOS GUI，确认
`./dist/agenterm-cli ui-snapshot` 可读；若 `./dist/agenterm-cli` 不存在，使用当前发布目录下对应 CLI 二进制（如 `~/.local/share/agenterm/releases/0.1.15-local-macos-aarch64/agenterm-cli`）代替，并在系统输入源中准备 ABC 与一个中文输入法。
若 `Control-Space` 不是本机切换快捷键，按提示从菜单栏手工切换，不得把快捷键未配置
记作产品失败。

```bash
set -eu

CLI="${CLI:-./dist/agenterm-cli}"
OUT=~/.local/share/agenterm/evidence/o1b-ime
mkdir -p "$OUT"

capture() {
  phase="$1"
  "$CLI" ui-action window-activate >/dev/null
  /usr/bin/defaults read com.apple.HIToolbox AppleSelectedInputSources \
    >"$OUT/$phase.input-source.txt"
  "$CLI" ui-snapshot >"$OUT/$phase.snapshot.json"
  "$CLI" screenshot -o "$OUT/$phase.png" >/dev/null
  /usr/bin/plutil -extract layout.status_bar.ime.label raw -o - \
    "$OUT/$phase.snapshot.json"
}

marker='agenterm-o1b-pbpaste-probe'
printf '%s' "$marker" | /usr/bin/pbcopy
test "$(/usr/bin/pbpaste)" = "$marker"

printf '切到 ABC，等待状态栏刷新后按回车：'
read -r _
abc_label="$(capture abc)"

printf '切到中文输入法，等待状态栏刷新后按回车：'
read -r _
zh_label="$(capture zh)"

printf '切回 ABC，等待状态栏刷新后按回车：'
read -r _
abc_return_label="$(capture abc-return)"

test "$abc_label" = 'IME: off'
test "$abc_return_label" = 'IME: off'
case "$zh_label" in
  'IME: '*' · native') ;;
  *) printf '中文状态判据失败: %s\n' "$zh_label" >&2; exit 1 ;;
esac

printf 'PASS: %s -> %s -> %s\n' \
  "$abc_label" "$zh_label" "$abc_return_label"
printf 'evidence: %s\n' "$OUT"
```

人工判据：

- 三份 JSON 的 `layout.status_bar.ime.bounds` 宽高非零，并含
  `event_position.{epoch,sequence}` 与 `focus.window_focused`。
- `abc` / `abc-return` 的 label 为 `IME: off`；`zh` 为
  `IME: <输入法本地化名称> · native`，不得为空、`off` 或 `latin`。
- `abc.png`、`zh.png`、`abc-return.png` 均非空且肉眼可读；状态段无重叠、截断。
- 三份 `*.input-source.txt` 依次旁证 ABC、中文、ABC；`pbpaste` 只证明剪贴板命令
  可用，不替代 IME 判据。
- 依次记录 `cargo test -p agenterm-platform --all-features ime::`、
  `cargo test --lib ui_geometry`、`./check.sh --quick` 的原始结果摘要。

O1b 闭环后，仅当 Candidate Promotion 后可见于 `/releases/latest` 的正式发布
`tag >= v0.1.15` 时再执行 **G1**：在不设置 `AGENTERM_ALLOW_UNSIGNED_PREVIEW` 的默认环境中
先探测 signed 资产。signed 为 200 时走 signed 安装验收，不期待 fallback 或 unsigned 信任警告；
signed 为 404/410 时才执行 §11，自动选择 unsigned-preview 并明确显示信任警告。两条分支都须
对账 distribution/variant/provenance，并确认升级后旧 server 只收到版本滞后提示、未被默认停止。
任何其它 HTTP、哈希或 provenance 错误均记录原文并停止验收。

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

```text
已达成：
- <ID>：<可观察结果>；证据=<命令摘要或仓库相对路径>

未完成：
- <ID>：<尚缺证据>；下一动作=<唯一可执行步骤>

阻塞：
- <ID>：<外部/人工边界>；安全结果=<保持未勾选或 fail-closed 行为>

改动 pathspec：plan/goal-agenterm-osx.md
真机证据目录：~/.local/share/agenterm/evidence/o1b-ime
```

---

## 8. 与 plan-v0.1.15 的关系

- **本文件** = OSX 机可转发的 goal / 派工 SSOT 切片（执行序 + 拍板 + 验收集）
- **plan-v0.1.15.md** = 全版素材与收敛树；O/G/P 细节与 §11 定因仍以彼为准
- 冲突时：拍板表以本文件 §1 与 plan §6 双写一致为准；细节叙事回 plan §11

---

## 9. 北极星（本 goal）

1. 灭 **O-fix** 红灯
2. 维持 **O1b** 状态栏 IME 真机 PASS
3. 顺手修 **con blackbox** 跨平台假红
4. 抽象只做 **有测的小切片**

每一步以可复现绿线证明存在；不报虚绩。

## 11. G1 真机重验：前置满足后执行脚本版本

目标：验证 **G1 的 unsigned fallback 分支**。在 macOS aarch64 真机、已有 AgenTerm server
存活、默认环境**没有** `AGENTERM_ALLOW_UNSIGNED_PREVIEW` 的条件下，验证 signed 资产确实为 404/410 后，
安装器自动选择完整的 unsigned-preview，强制输出信任警告，校验 SHA-256 与
provenance，写入一致的 installed record，并且不停止旧 server。

依赖图与安全边界：Candidate Promotion 后可见于 `/releases/latest` 的正式发布
`tag >= v0.1.15`、signed 为 404/410、preview 本体及两个伴随文件
可下载是本专项前置；安装、installed/provenance 对账和旧 PID 保持是串行链路。signed 为
200 时本专项不适用，应改走 §11.4（signed 分支脚本）闭环；signed 为其它状态、preview 任一文件
不可下载，或没有存活 server 时均在安装前停止，不把环境缺口记成产品 PASS。脚本只写
`~/.local/share/agenterm/evidence/...`，不写仓内文件。

从仓库根直接执行以下完整代码块；不要拆段执行。它明确使用当前仓库的
`./install.sh`，并通过 `env -u` 保证默认环境不含兼容确认变量：

```bash
bash <<'G1_REVERIFY'
set -u

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$HOME/.local/share/agenterm/evidence/g1-install-$RUN_ID"
mkdir -p "$OUT"
RESULT="$OUT/RESULT.txt"
: >"$RESULT"

fail() {
  printf 'FINAL=FAIL\nreason=%s\nevidence=%s\n' "$1" "${OUT/#$HOME/~}" \
    | tee -a "$RESULT" >&2
  exit 1
}

printf '%s\n' \
  'from repository root: env -u AGENTERM_ALLOW_UNSIGNED_PREVIEW AGENTERM_NO_LAUNCH=1 bash ./install.sh' \
  >"$OUT/00-command.txt"
{
  printf 'utc=%s\n' "$RUN_ID"
  printf 'os=%s\n' "$(uname -s)"
  printf 'arch=%s\n' "$(uname -m)"
  printf 'allow_unsigned_preview=unset\n'
  printf 'no_launch=1\n'
} >"$OUT/01-environment.txt"

[[ "$(uname -s)" == Darwin ]] || fail 'precondition: host is not macOS'
[[ "$(uname -m)" == arm64 ]] || fail 'precondition: host is not macOS aarch64'
[[ -f ./install.sh ]] || fail 'precondition: ./install.sh is missing'
command -v curl >/dev/null 2>&1 || fail 'precondition: curl is missing'
command -v python3 >/dev/null 2>&1 || fail 'precondition: python3 is missing'

LATEST_URL="$(curl -fsSL -o /dev/null -w '%{url_effective}' \
  https://github.com/mgttt/agenterm/releases/latest 2>"$OUT/02-latest.stderr.log")" \
  || fail 'preflight: cannot resolve latest release; preserve 02-latest.stderr.log verbatim'
TAG="${LATEST_URL##*/}"
VERSION="${TAG#v}"
python3 - "$VERSION" <<'PY' \
  >"$OUT/03-version-gate.stdout.log" 2>"$OUT/03-version-gate.stderr.log" \
  || fail 'preflight: tag is not a supported formal release tag'
import re, sys
match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)$", sys.argv[1])
if not match or tuple(map(int, match.groups())) < (0, 1, 15):
    raise SystemExit(f"required latest formal release >= v0.1.15, got v{sys.argv[1]}")
print(f"version_gate=PASS tag=v{sys.argv[1]}")
PY

BASE="https://github.com/mgttt/agenterm/releases/download/$TAG"
SIGNED="agenterm-$VERSION-macos-aarch64.zip"
PREVIEW="agenterm-$VERSION-macos-aarch64-unsigned-preview.zip"
http_status() {
  curl -sS -L --retry 3 -o /dev/null -w '%{http_code}' "$1"
}

SIGNED_HTTP="$(http_status "$BASE/$SIGNED" 2>"$OUT/04-signed-http.stderr.log")" \
  || fail 'preflight: signed asset probe had a transport failure'
{
  printf 'tag=%s\n' "$TAG"
  printf 'signed_asset=%s\n' "$SIGNED"
  printf 'signed_http=%s\n' "$SIGNED_HTTP"
} >"$OUT/04-release-preflight.txt"
case "$SIGNED_HTTP" in
  404|410) ;;
  200) fail 'preflight: signed asset exists; this fallback-only script is not applicable; use the signed branch in section 11.4' ;;
  *) fail "preflight: signed probe returned HTTP $SIGNED_HTTP; fail closed" ;;
esac

for suffix in '' '.sha256' '.provenance.json'; do
  status="$(http_status "$BASE/$PREVIEW$suffix" \
    2>>"$OUT/05-preview-assets.stderr.log")" \
    || fail "preflight: preview$suffix probe had a transport failure"
  printf '%s\t%s\n' "$PREVIEW$suffix" "$status" \
    >>"$OUT/05-preview-assets.tsv"
  [[ "$status" == 200 ]] \
    || fail "preflight: $PREVIEW$suffix returned HTTP $status"
done

pgrep -f '/agenterm( |$)' | sort -n >"$OUT/06-pids-before.txt" \
  || fail 'precondition: no live AgenTerm process; start the existing installation first'
ps -p "$(paste -sd, "$OUT/06-pids-before.txt")" -o pid=,etime=,command= \
  >"$OUT/07-processes-before.txt" 2>"$OUT/07-processes-before.stderr.log" || true

BEFORE_CLI="$HOME/.local/share/agenterm/current/agenterm-cli"
[[ -x "$BEFORE_CLI" ]] || fail 'precondition: current agenterm-cli is missing; cannot verify existing server-list'
"$BEFORE_CLI" server-list >"$OUT/08-server-list-before.txt" \
  2>"$OUT/08-server-list-before.stderr.log"
BEFORE_SERVER_LIST_RC="$?"
printf '%s\n' "$BEFORE_SERVER_LIST_RC" >"$OUT/08-server-list-before.exit-code"
[[ "$BEFORE_SERVER_LIST_RC" -eq 0 ]] \
  || fail "precondition: existing server-list probe failed with exit code $BEFORE_SERVER_LIST_RC"

if [[ -f "$HOME/.local/share/agenterm/current/installed.json" ]]; then
  cp "$HOME/.local/share/agenterm/current/installed.json" \
    "$OUT/09-installed-before.json"
else
  printf 'MISSING\n' >"$OUT/09-installed-before.txt"
fi

set +e
env -u AGENTERM_ALLOW_UNSIGNED_PREVIEW AGENTERM_NO_LAUNCH=1 \
  bash ./install.sh \
  > >(tee "$OUT/10-install.stdout.log") \
  2> >(tee "$OUT/11-install.stderr.log" >&2)
INSTALL_RC=$?
set -u
printf '%s\n' "$INSTALL_RC" >"$OUT/12-install.exit-code"
[[ "$INSTALL_RC" -eq 0 ]] \
  || fail "installer exited $INSTALL_RC; preserve 10/11 logs verbatim and stop"

CURRENT="$HOME/.local/share/agenterm/current"
[[ -f "$CURRENT/installed.json" ]] || fail 'postcondition: installed.json is missing'
[[ -f "$CURRENT/agenterm.provenance.json" ]] \
  || fail 'postcondition: retained provenance is missing'
cp "$CURRENT/installed.json" "$OUT/13-installed-after.json"
cp "$CURRENT/agenterm.provenance.json" "$OUT/14-provenance-after.json"
for executable in agenterm agenterm-cli agenterm-rh; do
  [[ -x "$CURRENT/$executable" ]] \
    || fail "postcondition: installed executable missing: $executable"
done
"$CURRENT/agenterm" --version >>"$OUT/15-version-after.txt" 2>&1 \
  || fail "postcondition: installed agenterm --version failed"
"$CURRENT/agenterm-cli" --version >>"$OUT/15-version-after.txt" 2>&1 \
  || fail "postcondition: installed agenterm-cli --version failed"
"$CURRENT/agenterm-rh" --version >>"$OUT/15-version-after.txt" 2>&1 \
  || fail "postcondition: installed agenterm-rh --version failed"

python3 - "$OUT/13-installed-after.json" "$OUT/14-provenance-after.json" "$TAG" \
  >"$OUT/16-record-assertions.txt" 2>"$OUT/16-record-assertions.stderr.log" \
  || fail 'postcondition: installed/provenance fields disagree'
import json, sys
installed = json.load(open(sys.argv[1], encoding="utf-8"))
provenance = json.load(open(sys.argv[2], encoding="utf-8"))
tag = sys.argv[3]
expected = {
    "tag": tag,
    "version": tag[1:] if tag.startswith("v") else tag,
    "channel": "macos-unsigned-preview",
    "distribution": "preview",
    "variant": "macos-aarch64-unsigned-preview",
    "os": "macos",
    "arch": "aarch64",
    "signed": False,
    "notarized": False,
}
errors = [f"installed.{k}: expected {v!r}, got {installed.get(k)!r}"
          for k, v in expected.items() if installed.get(k) != v]
checks = {
    "sha256": installed.get("sha256") == provenance.get("sha256"),
    "source_commit": installed.get("source_commit") == provenance.get("source_commit"),
    "source_tag": provenance.get("source_tag") == tag,
    "channel": provenance.get("channel") == "macos-unsigned-preview",
    "embedded_provenance": installed.get("provenance") == provenance,
}
errors.extend(f"record mismatch: {name}" for name, ok in checks.items() if not ok)
if errors:
    raise SystemExit("\n".join(errors))
print("record_assertions=PASS")
PY

grep -F "No signed macOS asset is available for $TAG;" "$OUT/10-install.stdout.log" >/dev/null \
  || fail 'log assertion: automatic fallback preface is missing'
grep -F 'automatically falling back to unsigned preview for installability.' \
  "$OUT/10-install.stdout.log" >/dev/null \
  || fail 'log assertion: automatic fallback result is missing'
grep -F 'Verified SHA-256:' "$OUT/10-install.stdout.log" >/dev/null \
  || fail 'log assertion: SHA-256 success is missing'
grep -F 'Provenance channel=macos-unsigned-preview' "$OUT/10-install.stdout.log" >/dev/null \
  || fail 'log assertion: provenance channel is missing'
grep -F 'WARNING: installing unsigned macOS preview archive' \
  "$OUT/11-install.stderr.log" >/dev/null \
  || fail 'log assertion: mandatory trust warning is missing from stderr'
grep -F 'A running AgenTerm process was detected' "$OUT/10-install.stdout.log" >/dev/null \
  || fail 'log assertion: running-server version-lag notice is missing'
grep -F "Disk install is $VERSION, but keep-server windows keep using the already-loaded PE." \
  "$OUT/10-install.stdout.log" >/dev/null \
  || fail 'log assertion: exact disk/server version-lag notice is missing'
printf 'log_assertions=PASS\n' >"$OUT/17-log-assertions.txt"

pgrep -f '/agenterm( |$)' | sort -n >"$OUT/18-pids-after.txt" \
  || fail 'postcondition: no AgenTerm process remains after install'
while IFS= read -r pid; do
  grep -qx "$pid" "$OUT/18-pids-after.txt" \
    || fail "postcondition: pre-existing AgenTerm PID $pid was stopped"
done <"$OUT/06-pids-before.txt"
ps -p "$(paste -sd, "$OUT/18-pids-after.txt")" -o pid=,etime=,command= \
  >"$OUT/19-processes-after.txt" 2>"$OUT/19-processes-after.stderr.log" || true
"$CURRENT/agenterm-cli" server-list >"$OUT/20-server-list-after.txt" \
  2>"$OUT/20-server-list-after.stderr.log"
AFTER_SERVER_LIST_RC="$?"
printf '%s\n' "$AFTER_SERVER_LIST_RC" >"$OUT/20-server-list-after.exit-code"
[[ "$AFTER_SERVER_LIST_RC" -eq 0 ]] \
  || fail "postcondition: server-list after install failed with exit code $AFTER_SERVER_LIST_RC"

{
  printf 'FINAL=PASS\n'
  printf 'tag=%s\n' "$TAG"
  printf 'signed_http=%s\n' "$SIGNED_HTTP"
  printf 'selected=unsigned-preview\n'
  printf 'trust_warning=PASS\n'
  printf 'sha256_provenance=PASS\n'
  printf 'installed_record=PASS\n'
  printf 'preexisting_server_pids_preserved=PASS\n'
  printf 'evidence=%s\n' "${OUT/#$HOME/~}"
} | tee "$RESULT"
G1_REVERIFY
```

### 11.1 成功日志判据

`10-install.stdout.log` 必须至少保留以下原文；版本号与 digest 按本次 release 变化：

```text
==> No signed macOS asset is available for v0.1.15;
==> automatically falling back to unsigned preview for installability.
==> Verified SHA-256: <64-hex>
==> Provenance channel=macos-unsigned-preview
==> Installed AgenTerm v0.1.15 to ~/...
==> Install record: ~/.../installed.json (version 0.1.15, distribution preview)
==> A running AgenTerm process was detected (server may still be the previous version).
==> Disk install is v0.1.15, but keep-server windows keep using the already-loaded PE.
```

`11-install.stderr.log` 必须包含完整信任警告块，至少可定位：

```text
==> WARNING: installing unsigned macOS preview archive
==> This binary is developer-preview level and is not Apple-signed
==> Trust only if you understand the signed-certificate gap.
```

signed URL 在 fallback 前产生的 `curl: ... 404` 是本场景的**期望旁证**，不是单独的
失败判据；最终以 `12-install.exit-code=0`、`16-record-assertions.txt`、
`17-log-assertions.txt`、旧 PID 全部仍在，以及 `RESULT.txt` 的 `FINAL=PASS` 为准。

### 11.2 期望失败日志与 fail-closed 分类

以下任一结果必须保持 `FINAL=FAIL`，不得手工下载、改 hash/provenance、设置
`AGENTERM_ALLOW_UNSIGNED_PREVIEW=1` 或停止旧 server
来换 PASS：

| 类别 | 应保留的代表原文 |
|------|------------------|
| `latest` 未达门槛 | `required latest >= v0.1.15, got ...` |
| signed 非缺失类错误 | `signed macOS asset download failed (HTTP ...); refusing unsigned fallback` |
| preview/伴随文件缺失 | `release asset/checksum/provenance is unavailable` |
| hash 不一致 | `SHA-256 verification failed for ...` |
| provenance 不一致 | `provenance verification failed:` |
| payload 不完整 | `release payload is missing agenterm-rh`（或其它必需二进制） |
| installed 对账失败 | `16-record-assertions.stderr.log` 的逐字段差异 |
| server 被停止 | `postcondition: pre-existing AgenTerm PID ... was stopped` |

### 11.3 失败原文保留规则

1. `10-install.stdout.log`、`11-install.stderr.log`、所有 `*.stderr.log`、HTTP 状态、
   exit code、安装前后 PID/server-list/installed/provenance 均为原始证据；首次生成后
   **不得覆盖、删行、排序、合并 stdout/stderr 或就地脱敏**。
2. 人工判断只追加到 `RESULT.txt` 或另建 `analysis.txt`，不得写回原始日志。失败后立即
   停止，不用第二次执行覆盖同一目录；重跑必须产生新的 UTC `RUN_ID` 目录。
3. 原始证据只留在 `~/.local/share/agenterm/evidence/...`，不提交仓库。若需把摘录写入
   `plan/**`，先按本仓脱敏规则把 home 改为 `~/...`，真实主机名/IP/凭据改为占位符，
   同时保留本地原文件及其 `shasum -a 256`，避免把“脱敏摘录”冒充原文。

### 11.4 signed 分支闭环脚本（建议）

目标：signed=200 时执行独立闭环，不走 unsigned fallback。与 §11.1/11.2 对称保留 fail-closed。
先决条件与 §11 相同（`latest`、`AGENTERM_NO_LAUNCH=1`、已有 server 且 `server-list` 成功）。

```bash
bash <<'G1_REVERIFY_SIGNED'
set -u

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$HOME/.local/share/agenterm/evidence/g1-install-signed-$RUN_ID"
mkdir -p "$OUT"
RESULT="$OUT/RESULT.txt"
: >"$RESULT"
export RESULT

fail() {
  printf 'FINAL=FAIL\nreason=%s\nevidence=%s\n' "$1" "${OUT/#$HOME/~}" \
    | tee -a "$RESULT" >&2
  exit 1
}

printf '%s\n' \
  'from repository root: env -u AGENTERM_ALLOW_UNSIGNED_PREVIEW AGENTERM_NO_LAUNCH=1 bash ./install.sh' \
  >"$OUT/00-command.txt"

LATEST_URL="$(curl -fsSL -o /dev/null -w '%{url_effective}' \
  https://github.com/mgttt/agenterm/releases/latest 2>"$OUT/02-latest.stderr.log")" \
  || fail 'preflight: cannot resolve latest release'
TAG="${LATEST_URL##*/}"
VERSION="${TAG#v}"
BASE="https://github.com/mgttt/agenterm/releases/download/$TAG"
SIGNED="agenterm-$VERSION-macos-aarch64.zip"

[[ "$(uname -s)" == Darwin ]] || fail "precondition: host is not macOS"
[[ "$(uname -m)" == arm64 ]] || fail "precondition: host is not macOS aarch64"

export TAG VERSION BASE SIGNED
python3 - "$VERSION" <<'PY' >"$OUT/03-version-gate.stdout.log" \
  2>"$OUT/03-version-gate.stderr.log" || fail 'preflight: required latest formal tag >= v0.1.15'
import re, sys
match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)$", sys.argv[1])
if not match or tuple(map(int, match.groups())) < (0, 1, 15):
    raise SystemExit(f"required latest formal release >= v0.1.15, got v{sys.argv[1]}")
print(f"version_gate=PASS tag=v{sys.argv[1]}")
PY

SIGNED_HTTP="$(curl -sS -L --retry 3 -o /dev/null -w '%{http_code}' "$BASE/$SIGNED" \
  2>"$OUT/04-signed-http.stderr.log")" \
  || fail "preflight: signed asset probe transport failure"
printf 'tag=%s\nsigned_asset=%s\nsigned_http=%s\n' "$TAG" "$SIGNED" "$SIGNED_HTTP" \
  >"$OUT/04-release-preflight.txt"
[[ "$SIGNED_HTTP" == 200 ]] || fail "preflight: expected signed=200, got $SIGNED_HTTP"

set +e
env -u AGENTERM_ALLOW_UNSIGNED_PREVIEW AGENTERM_NO_LAUNCH=1 bash ./install.sh \
  > >(tee "$OUT/10-install.stdout.log") \
  2> >(tee "$OUT/11-install.stderr.log" >&2)
INSTALL_RC=$?
set -u
printf '%s\n' "$INSTALL_RC" >"$OUT/12-install.exit-code"
[[ "$INSTALL_RC" -eq 0 ]] || fail "installer exited $INSTALL_RC"

"$HOME/.local/share/agenterm/current/agenterm" --version >>"$OUT/15-version-after.txt" 2>&1 \
  || fail "postcondition: installed agenterm --version failed"

python3 - "$HOME/.local/share/agenterm/current/installed.json" "$HOME/.local/share/agenterm/current/agenterm.provenance.json" "$TAG" \
  >"$OUT/16-record-assertions.txt" 2>"$OUT/16-record-assertions.stderr.log" || fail 'postcondition: fields disagree'
import json, sys
installed = json.load(open(sys.argv[1], encoding="utf-8"))
provenance = json.load(open(sys.argv[2], encoding="utf-8"))
tag = sys.argv[3]
expected = {
  "tag": tag,
  "version": tag[1:] if tag.startswith("v") else tag,
  "channel": "release",
  "distribution": "stable",
  "variant": "macos-aarch64",
  "os": "macos",
  "arch": "aarch64",
  "signed": True,
  "notarized": True,
}
errors = [f"installed.{k}: expected {v!r}, got {installed.get(k)!r}" for k, v in expected.items() if installed.get(k) != v]
checks = {
  "sha256": installed.get("sha256") == provenance.get("sha256"),
  "source_commit": installed.get("source_commit") == provenance.get("source_commit"),
  "source_tag": provenance.get("source_tag") == tag,
  "channel": provenance.get("channel") == "release",
  "embedded_provenance": installed.get("provenance") == provenance,
}
errors.extend(f"record mismatch: {name}" for name, ok in checks.items() if not ok)
if errors: raise SystemExit("\\n".join(errors))
print("record_assertions=PASS")

grep -F "No signed macOS asset is available for $TAG;" "$OUT/10-install.stdout.log" >/dev/null \
  && fail 'log assertion: unexpected unsigned fallback path in signed branch'
grep -F 'WARNING: installing unsigned macOS preview archive' "$OUT/11-install.stderr.log" >/dev/null \
  && fail 'log assertion: unsigned trust warning appeared in signed branch'
{
  printf 'FINAL=PASS\n'
  printf 'tag=%s\n' "$TAG"
  printf 'signed_http=%s\n' "$SIGNED_HTTP"
  printf 'selected=macos-aarch64.zip\n'
  printf 'record_assertions=PASS\n'
  printf 'evidence=%s\n' "${OUT/#$HOME/~}"
} | tee "$RESULT"
G1_REVERIFY_SIGNED
```

执行者需补充 `AgenTerm` 进程 PID 保留与 `server-list` 后置成功：`$OUT/18-pids-after.txt`、`$OUT/20-server-list-after.txt` 与 §12.2 共用即可。
## 12. 并行执行编排（gpt-5.6-sol, medium）

### 12.1 O1b 真机复测

- 目标：`ABC -> 中文 -> ABC` 三态 O1b 闭环（已 PASS；回归时按需复测）
- 阶段与验收：
  - 每阶段写 `json/<phase>.json`、`png/<phase>.png`、`input-source/<phase>.txt`
  - 三份 JSON 可解析并含 `event_position.epoch/sequence`、`focus.window_focused`
  - `layout.status_bar.ime.label/bounds` 存在；bounds 宽高均 > 0
  - `abc`/`abc-return` label 为 `IME: off`
  - `zh` label 匹配 `IME: <中文名> · native`，且不为 `IME: off`
  - PNG 非空且状态栏肉眼可读
- 证据目录：`~/.local/share/agenterm/evidence/o1b-ime-<UTC>`
- 失败分支：
  - 优先核对 `version/` 身份与 `protocol-info --running`
  - 若 identity 一致仍缺字段，归入快照投影回归问题
  - 若 `input-source` 未三态变化，归入手工输入法切换不达成

### 12.2 G1 真机安装闭环

- 目标：在 Candidate Promotion 后可见于 `/releases/latest` 的正式发布 `tag >= v0.1.15` 上，按实际分发状态闭环安装并写入 installed 记录
- 共享前置：保持默认环境（未设置 `AGENTERM_ALLOW_UNSIGNED_PREVIEW`）；安装前 `server-list` 成功且预置 server 存活
- 分支 S（signed）：signed 为 200 时直接安装 signed 资产；不得出现 unsigned fallback 或 trust warning；`installed.json` 须为 `distribution=stable`、`channel=release`、`variant=macos-aarch64`，并与保留的 provenance 内容一致
- 分支 U（unsigned fallback）：仅 signed 为 404/410，且 unsigned-preview 本体、`.sha256`、`.provenance.json` 均为 200 时触发 §11；必须自动 fallback 并输出 trust warning；成功判据直接引用 §11.1，失败关闭判据直接引用 §11.2
- 失败关闭：signed 为 200 以外且非 404/410，或所选资产/伴随文件不完整时，不安装、不跨分支降级
- 两分支共通判据：payload 含 `agenterm`、`agenterm-cli`、`agenterm-rh`；hash + provenance 校验通过；安装后 `server-list` 成功且预置 server PID 保留；`installed.json` 与实际资产一致
- 证据口径：保留失败原文；旧版 v0.1.14 的 `agenterm-rh` 缺失只作为历史阻塞，不再作为新 Candidate 的预期结果
- 证据目录：`~/.local/share/agenterm/evidence/g1-install-<UTC>`

### 12.3 con-blackbox 单项回归

- 目标：验证 `nonexistent_program_via_dash_e_exits_cleanly_instead_of_hanging` 不再挂死
- 复核命令：
  - `cargo test --test agenterm_con_blackbox nonexistent_program_via_dash_e_exits_cleanly_instead_of_hanging -- --exact`
- 判据：
  - 用例通过，退出码为 `0`
  - 10 秒内返回，不超时
  - 回执记录 stdout/stderr 与实际进程退出状态
