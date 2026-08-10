# CI 战况旁观分析(claude,2026-08-10 10:50)

| 字段 | 值 |
|------|-----|
| **性质** | 只读观察,**未做任何干预**;不改代码、不推提交、不动 codex 的工作面 |
| **观察对象** | v0.1.16 发布战役的 CI(workflow `ci.yml`),codex 在跟 |
| **数据来源** | `gh run list/view/cache list` 实拉,截至 run 31350654282(in_progress) |
| **配套文档** | `plan/ci-green-handoff.md`(claude 交接的目标与三个问题) |

---

## 1. 一句话

**只剩三个红,三个根因都已明确,其中一个是改一行数字;但最便宜的那个已被晾了四轮,
七分之五的提交压在最贵、且证据被污染的那个上。**

**更要紧的是 §7:三个红 lane 的构建缓存一条都不存在 —— `if: success()` 让红 lane
永远存不上缓存,于是"红 → 冷编译 → 更慢 → 迭代更少 → 继续红"闭环了 100 轮。
windows 一轮 20 分钟,而活更重的交叉编译 windows-aarch64 只要 3 分 14 秒。**

---

## 2. 事实基线

最近 100 次 `ci.yml` run(回溯到 2026-08-08 05:32Z)**零成功**。

但 run 31350187466(`ci: budget macOS ARM server cold start`)给出了整个战役里
第一次出现的新信息:

| lane | 状态 | 备注 |
|------|------|------|
| linux-aarch64 | 🟢 | |
| windows-aarch64 | 🟢 | |
| platform-contract ×4 | 🟢 | ubuntu / windows / macos-14 / macos-15-intel |
| **macos-x86_64** | 🟢 | **首次独立验证通过** —— 此前一直被 fail-fast 取消,从未单独跑完 |
| linux-x86_64 | 🔴 | |
| windows | 🔴 | |
| macos-aarch64 | 🔴 | |

即:六格矩阵里已经有五格能绿(x86_64-linux 除外),距离 candidate 只差三个修复。

---

## 3. 三个红的根因

### 3.1 linux-x86_64 —— 陈旧钉,一行数字,已红 4 轮无人碰 🟢易

失败步骤:`Audit cross-platform automation ownership`

```
{"code":"rh_backend","exit_class":"configuration",
 "message":"rh compile error: rh_fail: cross_platform_batch_inventory_count:7"}
```

- `scripts/rh/cross-platform-automation-audit.rh:108` 钉死 `batch_count == 5`
- 实际 `git ls-files -- '*.bat' '*.cmd'` 返回 **7** 个:
  `build.bat` / `check.cmd` / `lint.cmd` / `release.cmd` / `scripts/bootstrap.cmd`
  / **`rh-check.cmd`** / **`scripts/rh-check.cmd`**
- 后两个是 2026-08-06 的 `6b0b01ab`(*test(rh): dedicated rh-check suite*)带进来的,
  钉子从那天起就是错的

**为什么现在才炸**:linux-x86_64 此前一直死在更前面的 `Prove rh AOT pack pipeline`,
从未走到审计这一步。这就是交接文档里"陈旧钉在门首次真正跑通时集中爆发"那一族的又一例。

**修法**:把 `5` 改成 `7`,并按该脚本既有风格补两条
`actual_batch.contains("rh-check.cmd")` / `contains("scripts/rh-check.cmd")` 断言
(这是一个刻意的闭集断言,只改数字不补成员会削弱它的本意)。

**时间线**:自 `c26d9baa`(01:25Z)起连续 4 轮 CI 都是这一条,期间无人触碰。

### 3.2 windows —— 凶手已自报 🟡中

失败步骤:`Run quality gate`

```
rh_fail: process_timeout: <build-cache>\task-...\agenterm.exe
  "rh run <repo>/scripts/rh/remote-ui-smoke.rh --profile local
   --project-root <repo> --timeout-ms" after 310000ms
```

交接文档的问题 #2 已经从"不知道是哪个 task 超时"收敛成
**`remote-ui-smoke.rh` 在 Windows 上跑满 310s**。

- wave 9 的共享 pack target 缓存(`temp/agenterm-rh-pack-target-cg<rev>`,冷编译 30s→5s)
  已经上线,所以**这不是 pack 编译慢**
- wave 10 加的 args 预览标签正是为了让超时自报身份 —— 它起作用了
- 下一步应查 `remote-ui-smoke.rh` 本身在 Windows 上挂在哪(它此前也没在 AOT 下跑通过)

### 3.3 macos-aarch64 —— 真超时 + 被污染的证据,纠缠在一起 🔴难

失败步骤:`Prove native macOS Control Center lifecycle`

run 31349661037 的日志里**同时**出现:

```
STEP ... (全部步骤)
EVIDENCE control-center.macos-process-isolation
PASS: macOS caller-selected UDS, renderer-owned Retina evidence,
      no-activate/focus reuse, new epoch recovery, and isolation
{"code":"rh_backend", "message":"rh compile error: rh_fail:
 control_center_macos_server_timeout"}
##[error]Process completed with exit code 2.
```

**这是交接文档 §1 那个根因的指纹**:

- `scripts/rh/control-center-macos-smoke.rh:232` —— `wait_protocol()` 内 `throw code + "_timeout"`
- 调用点在 `:766`,传入 `"control_center_macos_server"` → 抛出串正是
  `control_center_macos_server_timeout`
- 整个主体包在 `:753` 的 `try { ... } catch (error) { failure = ... }` 里
- **AOT 语义**:被调函数内 `throw` = `rh_fail`(首错记录、run 必失败)+ 返回占位值继续执行,
  调用方的 `catch` **永不触发**

后果不是"误报",而是更糟的:

1. 那次 wait **确实到点了**(200×20ms),所以"macOS ARM 冷启动慢"这个方向本身不算错
2. 但超时之后的每一个 `STEP` / `EVIDENCE` 都是在**占位值**上跑出来的,全部无意义
3. `record_host_error`(`src/script_rh_host.rs:761`)首错保留 → 只会报**最早**那次超时
4. 于是日志**看上去是全绿的**(有完整 PASS 行),唯一信号是 exit code 和那一行首错串

**这就是为什么加预算像是在打地鼠**:加完这次超时的预算,下一轮换个地方超时,
拿到的仍是同一份"看着全绿"的日志。

**准确性边界**:最新一轮 31350187466 的 macOS 日志里已经**没有** PASS 行,
说明 `c26d9baa` 对该 smoke 的重写(-80 行)改变了行为形态。
所以上面的指纹我只能确认在 **31349661037** 上成立,不能断言现在仍是同一形态 ——
该 run 的完整日志要等整个 run 结束才拉得到。

---

## 4. codex 的投入分布(客观记录,非评价)

`c26d9baa` .. `26714c6c` 七个提交:

| 提交 | 指向 |
|------|------|
| `c26d9baa` fix(ci): restore cross-platform release gates | 跨平台门(15 文件,含 transpile.rs) |
| `54166202` ci: expose macOS worker crash diagnostics | macOS 诊断 |
| `878cb93c` fix(rh): release child id borrow before AOT calls | rh 引擎 |
| `6e42a4cd` ci: stream diagnosed Rh worker steps | macOS 诊断 |
| `ec7c2843` ci: budget full macOS Control Center journey | macOS 预算 |
| `83a9e0b0` ci: budget macOS ARM server cold start | macOS 预算 |
| `26714c6c` ci: cover loaded macOS ARM startup | macOS 预算 |

七分之五指向 macOS aarch64 的诊断与预算。同期 linux-x86_64 的一行数字钉连红 4 轮未动。

另有两个提交是 Windows PATH 经验的加入与回退(`b4659d1f` → `22bfc7bf` → `bd58358d`
落成 skill),属于工具链自建,与三个红无关。

---

## 5. 判断与建议顺序(**仅建议,未执行**)

按代价 / 信噪比排序,应该是:

1. **linux-x86_64**(改一个数字 + 补两条断言,五分钟)
2. **windows**(`remote-ui-smoke.rh` 已具名,单点排查)
3. **macos-aarch64**(最贵,且需要先解决证据污染)

理由不是"简单的先做",而是**信号纯度**:现在一轮 CI 要 15–20 分钟,同时糊着三个
互不相关的红。清掉前两个之后,macOS 成为唯一变量,每一轮的信号都直接归因,
迭代速度会有量级差别。

对 macOS 那条,建议在加预算之前先切断证据污染 —— 至少让脚本在首错之后**不再打印
PASS**(例如在最终 print 前查一次 host 错误状态),否则日志的"全绿"外观会持续误导。
根治仍是交接文档 §1 的两个选项:改脚本的否定用例形态(便宜),或转译器全函数
Result 化(大工程,不建议发布前做)。

---

## 6. 交接文档的三个问题,现状对照

| 交接文档 | 当时状态 | 现在 |
|---|---|---|
| #1 windows unit-tests(fresh_clone_rehearsal,AOT throw/catch 不解卷) | 本地复刻定位到唯一失败 | codex 在 `c26d9baa` 改了 `fresh-clone-rehearsal.rh`(-111 行)与其钉清单;windows 的红已换成 3.2 的超时 |
| #2 windows 证据阶段 process_timeout(310s) | 不知道是哪个 task | **已具名:`remote-ui-smoke.rh`** |
| #3 macos-aarch64 control-center-macos-smoke | 转译错误已修,运行期未知 | 运行期红,形态见 3.3 |
| (附注)macos-x86_64 从未独立验证 | 一直被 fail-fast 取消 | **已独立验证通过 🟢** |

---

## 7. 缓存饿死:比三个红加起来更值钱的一条

> 起因是一个问题:"有进展但不多,构建就没更好的服务了吗"。
> 拉了数据之后的结论是:**问题不在服务商,在缓存策略**。

### 7.1 数据

同一轮 run(31350187466)的各 lane 墙钟,与其 `cargo-target` 缓存是否存在:

| lane | 墙钟 | target 缓存 |
|------|------|------------|
| **windows-aarch64(交叉编译,活更重)** | **3m14s** | ✅ 2026-08-10 03:16 |
| linux-aarch64 | 3m37s | ✅ 2026-08-10 03:16 |
| macos-x86_64 | 3m22s | ✅ 2026-08-10 03:18 |
| macos-aarch64 | 8m22s | ❌ **不存在** |
| linux-x86_64 | 9m05s | ❌ **不存在** |
| **windows(原生 x86_64)** | **20m12s** | ❌ **不存在** |

`gh cache list --limit 100` 全表 22 条,`cargo-target-*` 只有三个前缀:
`cargo-target-v2-linux-aarch64`、`cargo-target-v2-macos-x86_64-apple-darwin`、
`cargo-target-v3-windows-aarch64-ci` —— **正好就是三个绿 lane**。

三个红 lane 不是缓存陈旧,是**一条都没有**。它们的 `cargo-home` 还停在
2026-08-04(windows-x86_64)和 2026-08-06(linux-x86_64)。

对照最有说服力:**交叉编译**的 windows-aarch64(cargo-xwin,活更重)3m14s 跑完,
**原生** windows-x86_64 要 20m12s。差的不是机器,是缓存。

### 7.2 根因

`.github/workflows/ci.yml` 里每一处保存都是同一个条件(共 6 处):

```yaml
- name: Save Windows x86_64 build target
  if: success() && github.event_name == 'push' && github.ref == 'refs/heads/main'
```

**红了就不存。** 于是形成闭环:

```
红 → 不存缓存 → 下一轮全量冷编译 → 慢 → 每小时迭代次数更少 → 继续红
```

最近 100 轮 CI 零成功,意味着这三个 lane **冷编译了 100 次**。
windows 那 20 分钟里绝大部分是在重编译整个 workspace,而不是在跑测试。

第二个坑:缓存总量已顶到 **10GB 上限**(22 条 active)。save key 里带
`${{ github.sha }}`,每轮每 lane 新铸一条 365–593MB 的条目;三个绿 lane 每次 push
吃掉约 1.4GB,**约 7 次 push 就把整个预算轮换一遍**。就算现在打开红 lane 的保存,
新存的也会被 LRU 迅速挤掉 —— 两个问题必须一起修。

(ci.yml 里 R1 那段注释已经意识到过 10GB 预算与 LRU 驱逐的问题,并为此把 target
路径瘦身成 deps/build/.fingerprint/incremental。方向对,但 per-sha key 的铸造速率
把瘦身省下来的空间又吃回去了。)

### 7.3 建议顺序(**仅建议,未执行**;这三条改的是 ci.yml,正是 codex 手上的文件)

1. **`if: success()` → `if: !cancelled()`**(6 处)
   - 测试失败的 target 目录是**完全有效**的:cargo 的 fingerprint 负责正确性,
     与测试过不过无关。编译产物不会因为断言失败而损坏。
   - 预计 windows 20m → 5m 上下;macos-aarch64、linux-x86_64 同理。
   - **一行改动,收益最大的一条。**

2. **止住 10GB 抖动**
   - save key 去掉 `${{ github.sha }}`(restore-keys 前缀匹配照常工作,save 覆盖同键),
     或定期 `gh cache delete` 清旧条目。
   - 不做这步,第 1 步的效果会被 LRU 吃掉一半。

3. **windows 那条红根本不需要走 CI**
   - 开发机就是 Windows。`remote-ui-smoke.rh` 本地直接可复现。
   - 本战役复刻 windows 单测门就是这么做的(`cargo test --all-features` + 门内
     的 12 个 `--skip`),循环从 20 分钟压到几分钟。

4. **macos-aarch64 是唯一真正需要"外部服务"的**
   - 但需要的不是更快的机器(8m22s 里大部分也是冷编译,第 1 步就能砍掉),
     而是**能进去看的机器**:在失败步骤前挂一个交互 shell(如 `action-tmate`),
     在 macOS ARM runner 上直接迭代,比 8 分钟一轮盲猜快一个量级。
   - 真要买机器(自托管 Mac / 云 Mac)是最后一步,不是第一步。

### 7.4 关于"换 CI 供应商"

在第 1、2 步做完之前**没有意义**。现在的瓶颈不是机器慢,是每轮都从零编译。
换到任何更快的 runner,省下的是编译时间的常数倍;修好缓存,省下的是编译本身。
先把自己造的坑填了,再谈要不要花钱。
