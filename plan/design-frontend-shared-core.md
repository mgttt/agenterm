# Frontend 共享核提取地图(P5 测绘结果)

> 2026-08-09。design-binary-size-and-reuse.md §4 队列 #1 的产出:对三大 frontend 面
> (Win remote_frontend 9,988 行 / Unix frontend mod 7,125 + render 4,350 行 /
> Win launcher 227 行)的完整重复度测绘。方法:功能区逐段对照 + fn 名集合求交 +
> 逐函数体比对。本文是后续提取工作的 SSOT;逐条落地后在 §5 勾销。

## 0. 总量结论

- Win remote 与 Unix embedded 两个 controller 之间有 **66 个同名函数**;渲染层
  (render.rs 与 GDI paint)才是真正平台特有的,controller 层不是。
- 共享层(src/frontend/ 22 个模块 + ui_geometry.rs + ui_snapshot.rs)已承接了
  状态/策略/几何的大头——**已有的下沉功课是认真的**;剩余重复集中在五类
  "编排层"(orchestration)代码,合计约 2,000+ 行双写。
- Win launcher(windows/frontend.rs)已 ~95% 提取完毕,是完成态样板。

## 1. 五大提取候选(按 重复行数 × 平价关键性 排序)

### #1 ui-snapshot 装配(~600 行双写,极高优先)
- Win `ui_snapshot_json()`(remote_frontend.rs:2498–3011)vs Unix
  `build_ui_snapshot_json()`(frontend/mod.rs:2704–3092);键集交集 ~88 个 JSON 键,
  连 `.expect("normal row has Add")` 的 panic 文本都相同。
- 已知真实分叉即平价缺陷:Unix 发 `caret`/`anchor`/`draft_length` 而 Win 不发
  (agent-human-parity-audit.md F7);Win 独有 `render_activity`/`capture_owned` 等。
- 提取形态:扩展 src/ui_snapshot.rs——`UiSnapshotSource` trait(tabs/active/layout/
  modal/focus/scrollbars)+ `build_ui_snapshot(source)`;宿主只实现 trait 与
  `extra` 钩子。
- **先立最便宜的护栏**:仿 ui_action_catalog.rs 的 include_str! 集合对比技术,
  加"两宿主快照键集必须相等(白名单豁免)"的单测,再动提取。

### #2 终端选区生命周期 + 多击链(~450 行,极高)
- 两面都驱动共享 `SelectionGestureState`,但通过两套平行单态化
  (`String,RemotePoint` vs `u64,TerminalPoint`)+ 共享模块内的平行函数对
  (`word_selection`/`remote_word_selection` 等)——**共享层自身携带着重复**。
- 双/三击判定谓词逐字节相同(remote 7046 / unix 3532)。
- 真实平价缺口:shift 扩选只有 Unix 有(`shift_extend_anchor`);Win 完全没有。
- 提取形态:selection.rs 升级为 `SelectionController<Id, Point>`(begin/drag/
  complete/cancel/autoscroll + 击链 + shift 扩选),合并 remote_* 函数对;
  平台侧只留指针捕获、像素→格映射、滚动机制、剪贴板写入。
- 平价钉:`ux-parity.remote-ui.selection`(remote-ui-smoke.rh:299–311)。

### #3 Modal 几何 + 命中测试(~500 行,高;**已实际漂移**)
- Win settings modal:width clamp(520,680)/height clamp(460,500)、preset 行 top+276;
  Unix render.rs `SettingsModalView`:480×380、theme 行 top+180——同一对话框两套数字,
  ui-input 点击 Settings/Apply 在两平台行为已经不同。
- 提取形态:`src/frontend/modal_geometry.rs`,7 个命名字段几何结构 +
  各自 `hit_test`,只吃 (client_width, client_height);`appearance_preset_grid`
  已证明该模式可行。渲染侧 View 结构退化为薄包装。

### #4 Sidebar 行命中 + 滚动模型(~300 行,高)
- `sidebar_max_offset`/`sidebar_offset`/`sidebar_scrollbar_state` 近逐字节相同;
  行命中两面同构但各缺一块:Win 缺 `TreeRowMode::Editing` 分支,Unix 缺
  `sidebar_geometry_generation` 失效护卫——双写导致的互补漏洞,合并后都补齐。
- 提取形态:ui_geometry.rs 增 `SidebarViewport` + `sidebar_row_hit -> SidebarRowHit`。

### #5 滚动条交互 + ui-input 指针合成(~450 行合计,中高)
- 半提取态:hit-test/thumb-drag 已共享,click/drag/end 编排 ×2 面 ×2 滚动条双写;
  ui-input press-loop 的注释两面逐字节相同。
- 提取形态:`ScrollbarController` 返回 `ScrollbarEffect` 命令枚举;
  `pointer_request_plan(request) -> Vec<SyntheticEvent>` 由宿主回放为各自事件类型。

### 候补:ui-action 双 match(~450 行)
已有机器检查的集合差测试守着,且 ARCHITECTURE.md 明确挂账为 L2 债、等表驱动设计
——是设计决定而非提取动作,排在五项之后。

## 2. 测绘中发现的具体缺陷(可独立修)

1. **元组契约反转**(潜伏漂移):server 上下文菜单几何,Win 侧构造
   `(frame, close, as_window)` 而 Unix 返回 `(frame, as_window, close)`;当前净行为
   恰好一致,但任何一侧重排即静默错位。合并到 server_strip_ui.rs 的命名字段结构即消除。
2. **F7**(已在 parity-audit 挂账):Win 快照缺 `caret`/`anchor`/`draft_length`。
   实际 scope(2026-08-09 勘察):Win composer 是原生 EDIT 控件,文本在控件里
   (`control_text(self.edit)`),选区必须新增 ControlWindow 契约方法
   `control_selection`(EM_GETSEL)+ Windows adapter 实现 + 测试替身;返回的是
   UTF-16 code-unit 偏移,须换算为字符索引才与 Unix `TextCursor` 语义对齐,
   且要先核查多行 EDIT 的 CRLF 与服务器草稿 `\n` 的往返一致性。独立成刀。
3. **shift 扩选 Win 缺失**(见 #2)。
4. **settings modal 几何漂移**(见 #3)。

## 3. 平台特有(不提取)

Win:ControlSpec/ControlId 注册表、SetWindowPos 布局、GDI paint、窗口消息分发、
resize/paste 工作线程、远端协议 tick/reconcile/poll。
Unix:XRGB 像素帧、HiDPI 缩放、字形栅格、vt100 归属、X11/Wayland 探测。

## 4. 既有平价护栏(动手前后都要绿)

- src/frontend/ui_action_catalog.rs 的 include_str! 双面扫描测试(最强既有闸门)
- tests/headless_ui_geometry.rs(快照形状)
- platform-ux-parity-smoke.rh 的 10 个证据 id;remote-ui-smoke.rh 的选区断言
- plan/platform-ux-parity-evidence-matrix.md(共享单点清单)
- **缺口**:没有"两宿主快照键集对等"与"modal 几何对等"的直接对比测试——
  这是 #1/#3 动手前应先补的最便宜护栏。

## 5. 进度

- [x] #1 前置护栏:快照键集对等测试(tests/snapshot_key_parity.rs,2026-08-09)——
  两宿主词汇表 192/180 键,共享 170;WIN-only 22(远端协议/原生控件机械)、
  UNIX-only 10(含 F7 三键)以显式 allowlist 钉住,新增键必须双侧同步或书面豁免;
  allowlist 条目本身有活性检查(键消失或对侧补齐都会报"删掉该条")。
- [ ] #1 UiSnapshotSource 提取
- [ ] #2 SelectionController 合并(含 shift 扩选补 Win)
- [ ] #3 modal_geometry.rs(含漂移收敛的产品决定:以哪套数字为准)
- [ ] #4 SidebarViewport + row_hit(互补漏洞补齐)
- [ ] #5 ScrollbarController + pointer_request_plan
- [x] 缺陷 1(元组契约):`layout_server_context_menu` 改返回泛型命名字段结构
  `ServerContextMenuRects<R>`(带 `map` 转换),两宿主 5 处消费点全部改字段访问,
  反序补偿代码消除(2026-08-09)。
