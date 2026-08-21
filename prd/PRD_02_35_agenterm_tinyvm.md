# PRD 02.35 — agenterm-tinyvm

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped · `[~]` partial · `[ ]` planned

```
agenterm-tinyvm (35)
├── eval(bytes)          [x]
├── iOS runtime boundary [x]
│   ├── interpret wasm      [x]
│   ├── JIT native code     [x]
│   ├── device-side AOT     [x]
│   └── dyn native loading  [x]
├── native wasm platform [~]
│   ├── tinyvm engine       [x]
│   ├── owned host ABI      [~]
│   ├── native I/O surface  [~]
│   └── H5/JS/WKWebView     [x]
├── game runtime         [~]
│   ├── persistent instance [x]
│   ├── start once          [x]
│   ├── per-call fuel       [x]
│   ├── memory budget       [x]
│   ├── table budget        [x]
│   └── game ABI            [~]
│       ├── standard .wasm cartridge [x]
│       ├── manifest compatibility    [x]
│       ├── core v1 imports           [~]
│       ├── init/tick/suspend/resume  [x]
│       ├── portable state snapshot   [x]
│       ├── bounded frame output      [x]
│       └── native module registry    [x]
├── real-game proofs               [~]
│   ├── Depth Well grid3d             [x]
│   ├── Paddle Guard indexed2d        [x]
│   ├── deterministic replay vectors  [x]
│   └── physical-device play          [ ]
├── slot-A
│   ├── control          [x]
│   ├── parametric       [x]
│   ├── locals           [x]
│   ├── memory           [x]
│   ├── i32              [x]
│   ├── i64              [x]
│   ├── f32              [x]
│   ├── f64              [x]
│   └── conv             [x]
├── host                 [x]
├── <100KiB>             [x]
├── slot-B               [ ]
└── non-goals
    ├── cu               [x]
    ├── chassis          [x]
    ├── #78              [x]
    ├── WASI             [x]
    ├── APE              [x]
    └── WAT              [x]
```

`eval(bytes)` → 值或错。程序是标准 `.wasm`。
宿主门是 import 表。未绑定即 trap。
槽 A = WASM 1.0 MVP 172 操作码。双绿。
核 strip `<100KiB`。

解释器路线是 iOS 发行边界驱动的架构选择，不是 PRD 为了缩小产品面而任意排除更快的执行方式。在目标 App Store 分发模型下，下载的 `.wasm` 只由 tinyvm 解释执行；不在设备上生成可执行原生代码，也不把下载模块编译或装载为原生动态代码。

这里排除的 AOT 特指“设备端把下载模块预编译成可执行原生代码”。开发或发布阶段把源码编译为标准 `.wasm`，以及应用本身的构建期 AOT，都不在此排除范围。`agenterm-dyn` 的本地原生动态调用面也不是 iOS tinyvm 执行路径。槽 B 暂停，直到目标分发模型与平台权限发生可验证的变化。

tinyvm 是自有 native WASM 平台的执行核，不是 H5 小游戏、JavaScript miniApp 或 WKWebView 容器。上层平台通过自有 host ABI 向 `.wasm` 提供经预算的原生渲染、输入、音频、时钟与存储面；不依赖 DOM、JavaScript 或 Web 容器语义。App Store 可接受性是该平台上层的独立发布门，不反向改写 tinyvm 的运行时架构。

JavaScriptCore 内部存在 WebAssembly 实现，但 Apple 公开的嵌入面是 `JSContext` 中的 JavaScript 执行，不是独立的原生 WASM module/instance API。JSC 可以作为后续对照基准或实验后端，但 tinyvm 是权威、可移植、可预算的 baseline；任何游戏都不得依赖 JSC 才能运行。

完整的 iOS 游戏运行底层验收树与依赖路径见 [`plan/goal-tinyvm-ios-game-runtime.md`](../plan/goal-tinyvm-ios-game-runtime.md)。

游戏卡带坚持使用标准 `.wasm` module；不增加 tinyvm 私有 opcode，也不把执行体改成私有二进制格式。核心能力由版本化的标准 function import namespace `tinyarcade:core/v1` 提供。Native 模块同样使用独立版本化 namespace，并且只有宿主 capability registry 明确注册的精确签名才能绑定；未知能力默认拒绝。C ABI v1.6 与 Swift package 已能为 bundled/reviewed 卡带注册这些能力，并在调用 app callback 前执行每生命周期配额；private-user 卡带保持 core-only。这样编译器、转换器与粉丝自制工具只需遵循卡带 ABI，而不依赖 tinyvm 内部实现。

官方远端目录和用户私有导入是两条不同的产品/审核路径：私有导入只进入用户自己的 app library，不自动公开或分发给其他用户；官方目录才走签名、复核、撤销与兼容性门。两条路径共同执行 WASM 验证、资源预算和 capability negotiation。

卡带兼容性以“标准 Wasm 文件 + 版本化平台契约”为准，而不是以某一版 tinyvm
内部实现为准。manifest 放在标准 custom section；core/native 能力都只表现为标准
function import，namespace、函数名、值签名和版本必须精确匹配。未来转换器可以只读
manifest/import table 就生成兼容性报告；未知 native module 默认拒绝，绝不把声明
本身视为装载原生代码的授权。`tinyarcade:core/v1` 的语义冻结，新增 native 能力使用
独立 `authority:module/vN`，不得暗改旧版本。

上述兼容性现在不是只写在文档里：Rust、CLI、C ABI 与 Swift 共用同一个静态
descriptor validator。它不实例化 module、不运行 start/init，就验证 manifest、标准
Wasm、lifecycle exports、core/native import 签名和 capability 对应关系，并输出 canonical
TAD1 描述。App 可以在安装前明确显示所需 native module；descriptor 只描述兼容需求，
不会授予 origin、签名信任或原生权限。

转换器也不再需要手拼 manifest bytes。`tinyvm cartridge attach-manifest` 接受任意
标准 producer 生成、尚无 TinyArcade manifest 的 `.wasm`，保持全部原始 bytes 为输出
前缀，只追加一个标准 custom section。native capability 不允许作为另一份手填参数，
而是从非 core function import namespace 自动去重、排序得出；输出前必须通过完整
descriptor，已有 manifest、ABI/lifecycle/import 不兼容、超过 2 MiB 或目标已存在均不
发布。Rust `CartridgeManifest::append_to_wasm` 提供同一 canonical encoder 给未来工具复用。

媒体边界不再假设所有游戏都是 Depth Well。`submit_render` 可提交严格有界的
`tinyarcade:grid3d/v1` 或 `tinyarcade:indexed2d/v1` 标准记录；后者提供完整
256 色调色板像素平面，默认 64 KiB 预算覆盖 256×240 与 320×200 经典画幅。
Swift `tickMedia` 先完整验证判别协议再向原生渲染层暴露数据，旧的 3D-only
`tick` 保持兼容。具体 Metal/Core Graphics 呈现仍属于 app host，因此 native
I/O surface 仍为 partial，而 bounded frame output 已具备通用 2D/3D 黑盒契约。
2D 卡带必须导入标准 core function `indexed2d_version() -> i32`；旧 runtime
会在实例化前拒绝未知 import，新 runtime 也拒绝未声明该 import 的 `TAI2`
输出，因此兼容性失败发生在装载/首个违规提交处而不是原生渲染崩溃。

iOS SDK 已把 2D 数据边界接到可直接复用的原生呈现面：严格验证后的索引帧可
展开为有界的 sRGB RGBA8 `CGImage`，或交给保持宽高比并使用 nearest filter
的 `TinyArcadeIndexed2DView`。自有 Metal renderer 仍可直接使用 palette/index
数据；UIKit/Core Graphics 类型不进入 WASM ABI。native I/O surface 仍标记为
partial，直到物理设备显示、输入与 audio-session 证据完成。

`tinyarcade:tones/v1` 同样是平台协议而不是某个游戏的音效代码：单批最多 16
个顺序事件、累计最多 4 秒，Rust 与 Swift 在调度原生工作前执行相同聚合校验。
iOS SDK 提供有界 PCM/WAV 合成与 `AVAudioPlayer` owner，默认使用服从静音键且
允许混音的 `.ambient` session；已有统一音频 owner 的 app 可关闭 SDK 的 session
管理。中断只停止、不重放过期反馈，退出游戏面时显式 deactivate。模拟器已用
Paddle Guard 的真实 launch tone 证明合成、播放、中断和释放；物理设备音频仍是
未完成证据，因此 native I/O surface 保持 partial。

C ABI v1.6 已消除 Rust trust/cache 与 iOS App 之间的断层。独立的单线程 cache
handle 和 Swift main-actor owner 接受 app 已完整接收的 bytes，在原子激活前复核
key/content 撤销、Ed25519、长度、SHA-256 与 embedded manifest；load/rollback
仍需对应 signed entry 并按当前 trust 再验证。cache 不拥有 URLSession 或 guest
network，失败加载也会清掉上一次待 copy 的结果。官方 catalog 的 transport、
metadata 与 deep link 协议见
[`docs/tinyarcade-catalog-transport-v1.md`](../docs/tinyarcade-catalog-transport-v1.md)：
Swift 严格限制 1 MiB/256 games、同源 HTTPS filename、display text/localizations、
signed-entry 字段与只选中不执行的 `tinyarcade://game/<game-id>`。JSON 不取得执行
授权；完整下载仍必须经过 signed entry 与 verified cache。Swift
`TinyArcadeHTTPSClientV1` 已交付 app-owned transport：只允许 HTTPS/200/指定 MIME，
拒绝 redirect，按声明长度和每个 delegate chunk 双重限流，cartridge 最终长度必须
与 signed entry 一致；timeout、active requests 和 queued waiters 全部有界，Task
取消会释放 in-flight 或 queued ownership。transport 成功不会自动 activate/open。
真实 hosted catalog、审核澄清与物理设备仍未交付，因此 distribution 继续为
partial。

离线发布端已形成独立且可复现的
[`tinyvm catalog build`](../docs/tinyarcade-catalog-publisher-v1.md) 契约：标准
`.wasm` 必须先通过 manifest/import、生命周期、媒体与 suspend/resume 确定性检查，
身份和 ABI/state 版本只从卡带内嵌 manifest 派生，再由独立的离线 Ed25519 catalog
key 绑定精确长度和 SHA-256。输出先写同级 staging directory，逐条用派生公钥重新
验签后才通过一次 rename 可见；已有目标不会覆盖，失败不会留下半发布目录，私钥
不会进入产物或日志。Native 扩展继续是标准 WASM 的版本化 function import，未来
粉丝转换器无需依赖 tinyvm 私有 opcode；官方上架则仍要求 app 预先审核并注册对应
native module。该工具不负责上传，真实站点与审核许可仍为 partial。

App 侧不再需要自行猜测 reviewed 安装顺序。`TinyArcadeReviewedLibraryV1` 把
catalog selection、受限 HTTPS、当前 trust/revocation、runtime/native capability
预检和 verified cache 组成一个 main-actor transaction：只有卡带已经成功以
`officialReviewed` 打开后才激活缓存。网络取消、并发选择、缺失 native module、
不兼容资源上限、篡改或撤销都不能把不可玩的对象变成 active；缓存重开仍按当前
trust 再验证。模拟器已用真实签名 Paddle Guard 证明完整路径，物理设备与真实站点
仍未完成。

`TinyArcadeSnapshotStoreV1` 已把裸 `suspend/resume` 补成可用于 iOS scene
lifecycle 的存档事务：每个 canonical game id 使用独立、限长、带 CRC 的版本化
binary envelope，同时保存 host-owned game clock；内部 snapshot 继续负责 ABI 与
state schema 兼容性。写入采用 atomic replace，目录不进 backup，文件使用
complete-until-first-authentication protection，并拒绝 symlink、非 regular 或超限
对象。损坏/不兼容存档不会在同一个已失败 runtime 上继续开新局，而是关闭候选、
删除坏文件并创建第二个 fresh runtime。模拟器已证明覆盖、恢复、损坏/超限回退与
symlink fail-closed；物理设备后台终止恢复仍未验证。

`TinyArcadePrivateLibraryV1` 已把“从 Data 打开”补成用户私有卡带的本地安装生命周期：
完整 bytes 先用 core-only private runtime 预检，再以 canonical
`game-id@version.wasm` 原子安装；枚举和打开重新检查身份、2 MiB 总上限、regular file
与 symlink 边界，目录最多 256 枚卡带且不进 backup。模拟器已证明真实 Paddle Guard
和 Depth Well 的导入、更新、排序、打开、删除，以及损坏、超限、live/dangling symlink
拒绝。它不包含文件选择 UI、网络上传或公开发布权限。

普通 tick 现在与 replay 使用同一输入事实：只接受 bit 0...8，并在 guest 执行前拒绝
倒退的游戏时钟；这种宿主参数错误不 latch、不改变 game state，修正后可以继续。
Swift `TinyArcadeGameSessionV1` 把最多 32 个 touch/keyboard/controller source 的完整
pressed set 合并，避免一个 source 松开时误清另一个仍按住的键；每帧只推进最多
250 ms（可配置上限 1...1000 ms）的 foreground game time，成功帧后才提交 clock，
并与 snapshot store 保存/恢复同一个 clock。`TinyArcadeFramePacerV1` 从
`CADisplayLink.timestamp` 等单调秒数保留亚毫秒余数地生成 delta，NaN、倒退和后台
大间隔均不改变 pacing baseline。app 在 scene 退后台时调用 `deactivateAndSave`，
session 会先清 inputs、进入 inactive 再保存，此后 input/tick 必然拒绝；回前台先
reset pacer 再 `activate`，第一帧为 0 delta。storage failure 不误判为 runtime failed，
suspend/runtime failure 则 latch session。snapshot 的 live/dangling symlink 均 fail closed。

标准化回放不另造游戏执行格式。`tinyarcade-replay-v1` 只保存精确卡带 SHA-256、
manifest identity、初始 portable snapshot、单调的 input/clock，以及每帧 render/audio
的长度和 SHA-256；执行时仍由原 `.wasm` 在 tinyvm 中生成完整输出并逐字节摘要核对。
8 MiB 总上限、1 MiB snapshot、65,536 steps 与媒体上限在分配前验证，回放 API 自己
绑定原始 `.wasm`，不依赖调用方记得先验 hash。Depth Well 与 Paddle Guard 已分别以
grid3d、indexed2d 和真实 tone 形成固定长度/SHA-256 golden；CLI 可从文本输入计划
确定性生成、验证且拒绝覆盖 `.tareplay`。未来 native import 仍走标准 versioned
namespace 与 registry；回放不会携带代码、授予 capability 或伪造 native side
effect，只会在同签名、确定性宿主行为下验证结果。
已加载 runtime 现在保留构造它的精确卡带 SHA-256，Swift 不必为了录制长期重复
持有 `.wasm`。main-actor owner 可从当前状态开始录制，让普通 tick 自动追加证据，
finish 返回标准 `.tareplay` Data；fresh runtime 可验证所有步骤。模拟器已证明真实
Paddle Guard 的录制、原子文件交换、逐字节复现、篡改拒绝和“相同 manifest、不同
WASM 字节”拒绝。验证会消费候选 runtime 状态，故产品 API 明确要求需要保留现场时
使用 disposable fresh runtime。

第二枚生产证明卡带 Paddle Guard 已消除“运行时只是为 Depth Well 特制”的可能：
它是 5,280-byte 严格 WASM MVP module，只导入八个 `tinyarcade:core/v1`
function，用 160×120 indexed frame、通用 input bits、impact/success/failure
tones 与 64-byte guest state 完成另一种街机循环。Rust 黑盒覆盖发射、移动、
护盾反弹、漏球、清场升级和逐字节恢复；iOS smoke 覆盖完整原生呈现。物理设备
仍未连接，因此 real-game proofs 和 native I/O surface 继续保持 partial。
