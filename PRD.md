# AgenTerm product tree

Status: active development
Platform: Windows shipped; Linux/macOS GUI and POSIX PTY in active delivery
  (see [`plan/plan-multiplatform-gui.md`](plan/plan-multiplatform-gui.md)).
Current default shell: Windows uses the real system `cmd.exe`; Unix GUI uses
`$SHELL` with `/bin/sh` fallback.
Future default-shell candidate: `agenterm-bash.exe`, only after its
clean-machine gate passes; no release version is committed

AgenTerm is a native terminal and local fleet workspace for people and
AI agents. The Windows GUI is the shipped human surface; Linux and macOS
share the same protocol, clients, and terminal kernel while their GUI
surface is delivered through continuous PTY/render abstraction. Its window is the bridge, the tab tree organizes the fleet, shells
are crew workspaces, and the local control plane lets people and agents observe
and steer the same state. Scripting reuses that public contract rather than
bypassing it. Human interaction and local CLI automation operate on the same
tabs, PTYs, drafts, settings, and observable state. A process exiting never
silently destroys its tab.

The repository ships two products, not one program with a light mode.
`agenterm` is the workbench described above. `agenterm-con` is an independently
packaged lightweight terminal host with no server, Fleet, mux, MCP or script
runtime; it reuses shared platform and UI-core mechanisms but owns its own
requirements, budget, tests and CI. Neither product may claim a capability or a
green status from the other's evidence.

The visual language favors industrial confidence over decoration: repeated
integer-grid spacing, solid right-angle connections, strict baseline
alignment, restrained colors, and explicit boundaries should make the fleet
feel precisely assembled and dependable.

AgenTerm competes by keeping the visible product simple and practical while
making the underlying system stable, observable, programmable, and open-ended.
New UI is justified by lower interaction cost, not feature count: advanced
power should prefer discoverable commands and programming interfaces, and
secondary controls should stay contextual or hidden by default when that keeps
the daily workspace quiet.

Terminal durability comes from deterministic two-dimensional state, not from
nostalgia. AgenTerm extends that contract from a character grid to the whole
agent fleet: humans and agents must be able to address, read, wait for, and
control the same tree nodes, focus, input, viewport, process lifecycle, and
rendered evidence precisely.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Documentation contract

This file is the canonical product entry point, product constitution, and
second-level tree. Linked `prd/PRD_*.md` modules own third-level requirements,
decisions, status, and acceptance detail. A requirement has exactly one owning
module; other documents link to it instead of copying it.

A module may be a subtree root when one product is large enough to need its own
second-level tree. Two subtrees exist today: `agenterm-con` (23, with 24-27) and
`agenterm-cu` (28, with 29-31). The root owns the product definition, boundary,
invariants and gates; the children own third-level requirements. A shared kernel
consumed by more than one product keeps its requirement in the owning shared
module and is referenced, not copied, from the subtree.

A subtree may be opened before a product exists, to hook newly accepted scope
into the tree so it cannot accumulate inside an unrelated module. Such a subtree
carries only `[ ]` requirements until its own evidence arrives; opening it is
not a version commitment.

Public version plans live under [`plan/`](plan/). They record sequencing,
dependencies, risks, decisions, and delivery history, but remain execution
projections rather than canonical product truth. Every accepted product scope
or capability-status change also belongs in its owning PRD module.
Living vs archived plan files: [`plan/README.md`](plan/README.md).

Current source layout (layers, bins, hot files, structural bans) lives in
[`plan/ARCHITECTURE.md`](plan/ARCHITECTURE.md). Version plans link to it; they
do not redraw a second living tree.

Stable public technical contracts that are too detailed for an owning PRD
module may live under [`docs/`](docs/). Every such specification links back to
exactly one owning PRD module; it defines interface semantics and conformance,
not independent product scope or shipped status.

Machine-readable shipped capability/evidence alignment lives in
[`prd/alignment-contract.json`](prd/alignment-contract.json).

Later executable names, intelligence approaches, and external-runtime
integrations are gated product or research hypotheses unless a roadmap
milestone explicitly assigns them to a release. Their presence in the product
tree does not promise a version or implementation strategy.

## Product tree

```text
AgenTerm — local agent & process fleet work OS
│
├─ 终端与运行时
│  ├─ 01 Terminal runtime        ConPTY、渲染、输入、选择、滚动、性能
│  ├─ 03 Default shell          agenterm-bash.exe 策略与兼容门
│  └─ 16 tmux/RMUX compatibility  兼容矩阵、显式差异、一致性证据
│
├─ 工作区与权威
│  ├─ 06 Human workspace        Tabs、composer、settings、持久化、状态栏
│  ├─ 02 Executable family      二进制角色、边界、预算、sidecar 归属
│  ├─ 05 Fleet multiplexer      agenterm cli mux（tmux/RMUX 兼容控制）
│  ├─ 08 Observable Fleet       纪元/序列日志、读取、等待、缺口、重启
│  └─ 07 Agent control plane    观察、控制、协议、身份、确定性等待
│
├─ 自动化与智能
│  ├─ 10 Rust host + Rhai       本地无限制运行时、监督、注册、审计；Agent 权限在其上层
│  ├─ 11 MCP orchestration      agenterm cli mcp（只读 MCP → 受管工具/流/调度）
│  ├─ 12 Specialized intelligence  agenterm-ai.exe（未指派研究方向的证据门）
│  └─ 13 Local LLM gateway      agenterm-llm-gateway.exe（受管网关假设门）
│
├─ 分发与治理
│  ├─ 04 Optional components    agenterm-softmgr.exe（签名清单/安装/更新/回滚）
│  ├─ 09 Self-hosted dev loop   构建、暂存、更新可见性、安全迭代
│  ├─ 14 Research provenance    源码审查、许可、来源、独立实现
│  ├─ 15 Command line           agenterm cli（公共命令/发现/输出契约）
│  └─ 17 Delivery and quality   构建、测试、产物、发布门、回归预算
│
├─ 平台抽象
│  └─ 20 Native platform        Win/macOS/Linux 原生适配（窗口/输入/IME/DPI/剪贴板/字体）
│
├─ agenterm-con（第二产品子树）
│  └─ 23 agenterm-con           产品定义、边界、不变量、安全失败（子树根）
│     ├─ 24 Terminal & rendering   PTY、VT 行级 damage、present、字形、ISA、渲染性能
│     ├─ 25 Workspace & input      Tab 树、chrome、composer、滚动条、选择、剪贴板
│     ├─ 26 Control & public CLI   agenterm-con cli、ATC1 帧、JSON 契约、证据发布
│     └─ 27 Package & delivery     独立 package、con-* profile、<1 MiB 预算、独立 CI
│
├─ agenterm-cu（computer-use 子树 · 全部 planned）
│  └─ 28 agenterm-cu            自有 computer-use 底座：定义、边界、不变量、晋升门
│     ├─ 29 Command surface       抽象命令集、洋葱分层、结构化控件树、确定性等待
│     ├─ 30 Targets & transports  current/ssh/rdp/vnc 目标族、平台后端、会话模型
│     └─ 31 Authorization & audit 高危面授权、审计、拒绝语义、交付门
│
└─ 未来面（里程碑 / 灵感）
   ├─ 18 Focused product roadmap  版本归属、里程碑门、未来产品泳道
   ├─ 19 Inspiration backlog      灵感花园、北极星层、晋升路径（非 shipped）
   ├─ 21 Control Center          agenterm-cc（独立次级工作区：Cockpit/workflow/extension/info）
   └─ 22 Decentralized network   agenterm-net（libp2p/IPFS 独立成熟）
```

### Module index

| # | 模块 | 一句话 |
|---|------|--------|
| 01 | [Terminal runtime](prd/PRD_02_01_terminal_runtime.md) | ConPTY、渲染、输入、选择、滚动、性能 |
| 02 | [Executable family](prd/PRD_02_02_executable_family.md) | 二进制角色、边界、预算、sidecar 归属 |
| 03 | [Default shell (`agenterm-bash.exe`)](prd/PRD_02_03_default_shell.md) | 真实 Bash 策略与兼容门 |
| 04 | [Optional component lifecycle (`agenterm-softmgr.exe`)](prd/PRD_02_04_optional_components.md) | 签名清单、安装、更新、回滚、供应链安全 |
| 05 | [Fleet multiplexer (`agenterm cli mux`)](prd/PRD_02_05_fleet_multiplexer.md) | tmux/RMUX 兼容控制 |
| 06 | [Human workspace](prd/PRD_02_06_human_workspace.md) | Tabs、composer、settings、持久化、状态栏、交互设计 |
| 07 | [Agent control plane](prd/PRD_02_07_agent_control_plane.md) | 观察、控制、协议、身份、确定性等待 |
| 08 | [Observable Fleet event core](prd/PRD_02_08_observable_fleet.md) | 纪元/序列日志、读取、等待、缺口、重启、消费者 |
| 09 | [Self-hosted development loop](prd/PRD_02_09_self_hosted_development.md) | 构建、暂存、更新可见性、安全迭代 |
| 10 | [Rust host + script engines (Rhai/rh/lua/qjs)](prd/PRD_02_10_rhai_scripting.md) | 本地无限制运行时、监督、注册、审计、providers；多引擎家族见该文档「Script engine family」 |
| 11 | [MCP and agentic orchestration (`agenterm cli mcp`)](prd/PRD_02_11_mcp_orchestration.md) | 只读 MCP 先行，再受管工具、流、调度 |
| 12 | [Lightweight specialized intelligence (`agenterm-ai.exe`)](prd/PRD_02_12_specialized_intelligence.md) | 未指派可选智能研究方向的证据门 |
| 13 | [Local LLM gateway (`agenterm-llm-gateway.exe`)](prd/PRD_02_13_llm_gateway.md) | 未指派受管网关假设的安全门 |
| 14 | [Research provenance and clean-room boundary](prd/PRD_02_14_research_provenance.md) | 源码审查、许可、来源、独立实现 |
| 15 | [Command line (`agenterm cli`)](prd/PRD_02_15_command_line.md) | 公共命令、发现、输出契约、生命周期 |
| 16 | [tmux/RMUX compatibility](prd/PRD_02_16_tmux_rmux_compatibility.md) | 兼容矩阵、显式差异、一致性证据 |
| 17 | [Delivery and quality](prd/PRD_02_17_delivery_quality.md) | 构建、测试、产物、发布门、回归预算 |
| 18 | [Focused product roadmap](prd/PRD_02_18_roadmap.md) | 版本归属、里程碑门、未来产品泳道 |
| 19 | [Inspiration backlog and future vision](prd/PRD_02_19_inspiration_and_future_vision.md) | 灵感花园、北极星层、晋升路径（非 shipped） |
| 20 | [Native platform abstraction](prd/PRD_02_20_native_platform.md) | Win/macOS/Linux 窗口/输入/IME/DPI/剪贴板/字体契约 |
| 21 | [Control Center (`agenterm-cc`)](prd/PRD_02_21_control_center.md) | 独立次级工作区：Fleet cockpit/workflow/extension/info 投影 |
| 22 | [Decentralized network (`agenterm-net`)](prd/PRD_02_22_decentralized_network.md) | libp2p 身份、IPFS 内容寻址、存储、传输、服务集成契约 |
| 23 | [Lightweight terminal host (`agenterm-con`)](prd/PRD_02_23_agenterm_con.md) | 子树根：产品定义、边界、不变量、跨面证据、安全失败 |
| 24 | [`agenterm-con` terminal and rendering](prd/PRD_02_24_con_terminal.md) | PTY 会话、VT 行级 damage、present、字形、ISA、渲染性能证据 |
| 25 | [`agenterm-con` workspace and input](prd/PRD_02_25_con_workspace.md) | Tab 树权威、chrome、composer、滚动条/分隔条、选择与剪贴板 |
| 26 | [`agenterm-con` control and public CLI](prd/PRD_02_26_con_control_cli.md) | 公共命令集、`ATC1` 传输、JSON 契约、快照/截图证据发布 |
| 27 | [`agenterm-con` package and delivery](prd/PRD_02_27_con_delivery.md) | 独立 package、`con-*` unwind profile、体积预算、独立 CI、体积历史 |
| 28 | [Computer-use foundation (`agenterm-cu`)](prd/PRD_02_28_agenterm_cu.md) | 子树根：自有 computer-use 底座的定义、边界、不变量、晋升门（全部 planned） |
| 29 | [`agenterm-cu` command surface](prd/PRD_02_29_cu_command_surface.md) | 抽象命令集、洋葱分层契约、结构化控件树观察、确定性等待 |
| 30 | [`agenterm-cu` targets and transports](prd/PRD_02_30_cu_targets_transports.md) | `current`/`ssh`/`rdp`/`vnc` 目标族、平台后端、会话与进程模型 |
| 31 | [`agenterm-cu` authorization and safety](prd/PRD_02_31_cu_authorization_safety.md) | 高危能力面的授权模型、审计、拒绝语义、交付门 |

## Non-negotiable invariants

- Exiting a child process does not remove its tab.
- Normal application restart preserves workspace structure and metadata while
  honestly restarting each PTY process.
- A live tab is not destroyed without an explicit close and confirmation.
- Tab IDs remain stable for the lifetime of the tab; indexes may change.
- Agent-facing state is machine-readable and actions can be verified without
  arbitrary sleeps.
- `agenterm-rh` exposes one unrestricted local runtime with the invoking
  user's operating-system authority. Rhai has no permission tier, approval
  profile, protected-path/process/endpoint list, or restricted substitute for
  an unshipped API; Agent policy belongs to the separate Agent/harness layer.
- tmux/RMUX names are used only where behavior is compatible. Unsupported
  behavior returns an error rather than pretending to succeed.
- AgenTerm does not silently download or bundle fonts. `Sarasa Fixed SC`
  (SIL OFL 1.1) is the recommended optional CJK monospace font.

## Current acceptance gate

Run `.\lint.cmd` for fast local feedback and `.\check.cmd` for ordinary
changes. A change is ready only when repository static lint, formatting,
Clippy with warnings denied, production Rhai checks, unit tests, `dist/`
artifact generation, CLI smoke, and semantic UX smoke all pass. Rendering
changes additionally require
`screenshot` or `screenshot-pane` inspection.

`agenterm-con` has its own ordinary gate: the matching custom-std
`con-release-fast` Clippy, unit, public GUI black-box, panic-containment and
artifact build path, plus its six compile cells. Candidate preflight requires a
successful run of both `.github/workflows/ci-agenterm.yml` and
`.github/workflows/ci-agenterm-con.yml` at the exact source SHA; one product's
green status never substitutes for the other's.

An unpublished release candidate uses
`.\check.cmd --release --include-stress` on a clean commit and must emit one
complete qualification receipt bound to the exact candidate bytes. The
independent `agenterm-rh task run package-qualified` step may only copy
those byte-identical qualified artifacts; it does not rebuild. A non-publishing
release rehearsal must validate the candidate, receipt, package manifest, and
remote workflow contract before publication is considered. Creating or
pushing a release tag, or creating a public GitHub Release, still requires the
user's explicit approval. The deleted v0.1.9 GitHub Release and its retained
immutable tag must not be overwritten; the historical server-loss hotfix uses SemVer
`0.1.9+hotfix.1` and tag `v0.1.9+hotfix.1`, with public release title
“AgenTerm v0.1.9.1 Hotfix”. Versions 0.1.7 and 0.1.8 remain historical
baselines; v0.1.7 is internal-only and must never produce a tag or public
GitHub Release. Versions 0.1.12 and 0.1.13 were planned and worked on but
never publicly released: their candidates were abandoned and superseded, so
the public sequence runs v0.1.11 → v0.1.14. Their `plan/plan-v0.1.1{2,3}.md`
records remain authoritative for what was built and why, but neither version
has a tag or GitHub Release, and neither should be treated as shippable.
