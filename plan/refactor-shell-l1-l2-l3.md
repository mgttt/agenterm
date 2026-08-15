# Plan tree: 薄 L1 冻壳 + 打包合成成品（不是再编译）

| 字段 | 值 |
|------|-----|
| **状态** | active — 执行投影 |
| **结构 SSOT** | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| **产品归属** | [`prd/PRD_02_10_rhai_scripting.md`](../prd/PRD_02_10_rhai_scripting.md) Layered deployment；[`prd/PRD_02_18_roadmap.md`](../prd/PRD_02_18_roadmap.md) M15 |
| **L1 面** | [`shell-l1-surface.json`](shell-l1-surface.json) |
| **W1 合成器** | [`scripts/shell-compose-product.py`](../scripts/shell-compose-product.py) |

## 0. 对的经济模型（类比，不用 libtcc）

不是「一个 JVM 里焊死全世界」。是 **两套循环**：

```text
少见、贵     为 6 个 OS×ISA 各编一份很薄的 L1 loader
             编完就冻住。只有跳板/呈窗/PTY/IPC/加载器不够时才再编。

常见、便宜   拿已经存在的 L1 二进制 + L2 码 + 可选 L3 包
             打成能测、能发的成品。
             这里不 rustc、不 cargo、不六格 Candidate。
```

L1 不是一个文件，是 **六份冻壳**（win/lnx/osx × x86_64/aarch64）。  
日常加速靠第二条循环占满。

成品：

```text
product-<id>.tgz
  manifest.json          各层 SHA、格名、禁止 cargo 的声明
  l1/<cell>/loader       原样拷贝冻壳，字节不得变
  l2/…                   宿主 ABI / 资源表 / 可移植落实
  l3/…                   应用包（.agp 或等价物）
```

测的是「这份包能否被声明的 L1 加载」，不是再编译 workspace。

## 1. 三层

| 层 | 是什么 | 日常怎么动 |
|----|--------|------------|
| **Shell-L1** | 每格一份薄 loader | 几乎不动；动则六格编译 |
| **Shell-L2** | 应用看见的 ABI + 跨 OS 落实（表/包/cu 插件） | 换文件再打包 |
| **Shell-L3** | 应用（class/jar 那一层） | 换包再打包 |

L3 只写能力名。L3 里出现 `libSystem` / `libc.so` / `kernel32` = 失败。  
L2 若还是半个 `agenterm.exe`，打包循环是假的。

命名用 **Shell-L\***，避免和脚本 L1/L2/L3 混称。

## 2. 波次

```text
W0  点名 L1 路径面（已做）
W1  打包循环证明：合成器 + 黑盒测试，过程中不得调用 cargo（本增量）
W2  Host ABI 名字表；cu 登记为 L2
W3  第一份真实 L2 产物走合成器进「成品」夹具
W4  接 v0.1.18 `.agp` 当 L3，不重开轨 A
```

## 3. W1 验收（本增量必须绿）

- [`scripts/shell-compose-product.py`](../scripts/shell-compose-product.py) 只读已有 L1 字节 + L2/L3 文件树，写出确定性 tar。
- [`scripts/shell-compose-product-test.py`](../scripts/shell-compose-product-test.py) 用合成夹具跑两遍：
  - 子进程环境 **没有** `cargo` 可执行文件仍能合成；
  - 六个 cell 的 `l1/<cell>/loader` SHA 与输入相同；
  - 两遍成品 SHA 相同。
- 本波 **不** 把现行 `agenterm` PE 拆成真 loader。证明的是循环，不是已经瘦身。

## 4. 非目标

- 用 libtcc / 嵌编译器
- Electron 进正式 PE
- L3 直接 `dlcall`
- 本波改 GitHub Actions 矩阵
- 重开 ape 编译拆分当热更

## 5. 现行对照

今天改 frontend 仍可能拖进同一份 PE 的编译。W1 只建立 **旁边的** 打包证明，让后面把真 L2/L3 往这条循环迁。
