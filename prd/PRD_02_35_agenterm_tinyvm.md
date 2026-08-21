# PRD 02.35 — agenterm-tinyvm

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped · `[~]` partial · `[ ]` planned · `[–]` intentionally excluded

```
agenterm-tinyvm (35)
├── eval(bytes)          [x]
├── iOS runtime boundary [x]
│   ├── interpret wasm      [x]
│   ├── JIT native code     [–]
│   ├── device-side AOT     [–]
│   └── dyn native loading  [–]
├── native wasm platform [~]
│   ├── tinyvm engine       [x]
│   │   └── decode complexity budget [x]
│   ├── owned host ABI      [~]
│   ├── native I/O surface  [~]
│   └── H5/JS/WKWebView     [–]
├── game runtime         [~]
│   ├── persistent instance [x]
│   ├── start once          [x]
│   ├── per-call fuel       [x]
│   ├── explicit guest call stack [x]
│   │   ├── host-owned call-depth ceiling [x]
│   │   └── host-owned activation-slot ceiling [x]
│   │       └── fallible execution-stack growth [x]
│   ├── memory budget       [x]
│   ├── table budget        [x]
│   ├── deterministic execution stats [x]
│   │   └── call/activation peak telemetry [x]
│   └── game ABI            [~]
│       ├── standard .wasm cartridge [x]
│       ├── manifest compatibility    [x]
│       ├── core v1 imports           [~]
│       ├── init/tick/suspend/resume  [x]
│       ├── portable state snapshot   [x]
│       ├── bounded frame output      [x]
│       │   └── recyclable host buffers [x]
│       ├── native module registry    [x]
│       ├── bounded in-place host dispatch [x]
│       ├── App Store bundled-only gate [x]
│       └── machine host profile      [x]
│           └── catalog profile binding [x]
├── real-game proofs               [~]
│   ├── Depth Well grid3d             [x]
│   ├── Paddle Guard indexed2d        [x]
│   ├── deterministic replay vectors  [x]
│   ├── development WebKit differential [x]
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
│   ├── conv             [x]
│   └── standard proposal profile [~]
│       ├── bulk memory copy/fill [x]
│       ├── bulk memory passive lifecycle [x]
│       ├── sign extension proposal [x]
│       ├── nontrapping float-to-int [x]
│       ├── multi-value proposal [x]
│       ├── single-table funcref profile [x]
│       ├── multiple defined funcref tables [x]
│       └── tail-call proposal [x]
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
tinyvm 的产品定位是事实上的、标准优先的跨平台 WebAssembly VM；TinyArcade
只是第一个 embedding 和持续验收负载，不是引擎能力的边界。任何游戏便利能力都必须
通过标准 Wasm 或显式、版本化的 host import 表达，不能演化为游戏专用私有字节码。
槽 A 以标准 WebAssembly 为持续兼容目标，而不是停在自定义 VM 或永久冻结为 MVP。
当前 scalar MVP 面双绿，并已原生接受完整的 single-memory / MVP-funcref
bulk-memory proposal：copy/fill、passive data/element、init/drop、table.copy 与
DataCount，以及 sign-extension、non-trapping conversion 和 multi-value proposal。
Multi-value 包含多结果函数、s33 type-index block signature、带参数 block/loop/if 和
多值 branch；validator 控制帧只引用已经预算的 type section，不按嵌套层次复制签名。
当前 reference-types 面先完成标准 single-table `funcref` 闭环：reference 值、局部变量、
全局变量、typed select、`ref.null`/`ref.is_null`/`ref.func`、table get/set/grow/size/fill，
以及 expression element segment flags 4..7。`externref`、typed function references 和 GC
尚未进入接受 profile，会在 load gate 明确拒绝，不以私有编码代替。
在此基础上，模块可定义多张 `funcref` table；所有 table instruction、`call_indirect`、
active element segment 和跨表 `table.copy` 均使用标准 table index。初始与动态 table
预算按实例中所有表的元素总数计算，不能用多张小表绕过宿主上限。当前仍不接受 imported
tables；定义表的 export 会完整验证，但产品 embedding 暂只公开 function lookup。
标准 tail-call proposal 的 `return_call` 与 `return_call_indirect` 已进入 profile；执行器以
trampoline 替换当前 activation，长尾调用链不会消耗 Rust/iOS native stack。普通非尾调用
也使用显式、fallible VM activation vector，不再递归进入 Rust；direct/indirect 调用统一受
512 activation 与 1,048,576 aggregate live-slot 上限约束，debug/release 行为一致。尾调用
也能落到版本化 native import，但 imported table
仍需跨 instance 的 store-level function identity 后才能合规共享。
执行期的 operand/control 增长会先检查 live-slot/operand 上限并 `try_reserve`，然后才执行
指令；call/tail-call 参数与函数结果先完整 fallible allocation，再从源 stack 移走。
`br_table` targets 在 decode 时进入每函数的扁平、不可变 arena，执行指令只保存 range 并
直接借用；branch value 通过原 stack 内 copy 保留，不会在循环热路径 clone guest-sized
vector、触发二级分配或为每次分支建立临时 `Vec`。
游戏 embedding 的 core imports 与 iOS C native callbacks 使用同一有界 in-place host
door：最多 16 个 i32 参数/结果在固定栈数组中传递，VM 在进入 app callback 前为 suspended
operand stack 或顶层结果完整 `try_reserve`。嵌套 core/native dispatch 直接把 inline 结果
写回已预留的 caller stack；输入、时钟、RNG、媒体提交、状态保存/恢复和 C callback 不再
为每次 dispatch 建立临时 heap `Vec`。通用 Rust returning-callback API 仅作为兼容层保留。
其它标准 proposal 必须逐项补齐解码、验证、执行、资源预算和独立引擎差分证据后进入
compiler profile。
核 strip `<100KiB`。

不可信卡带的 2 MiB 文件上限之外还有统一 decode complexity budget：section entry、
type value、local、decoded instruction、element index 与 `br_table` target 合计最多
262,144 条放大分配记录。所有 guest count 在 reserve 前扣减并使用 fallible allocation；
标准 section 必须唯一、按序且完整消费 payload。极小文件谎报数十亿条目只能得到
decode error，不能触发 iOS 进程 OOM abort。该限制是 TinyArcade 的标准 WASM compiler
profile，不改变 `.wasm` 格式或增加私有 opcode。

解释器路线是 iOS 发行边界驱动的架构选择，不是 PRD 为了缩小产品面而任意排除更快的执行方式。在目标 App Store 分发模型下，下载的 `.wasm` 只由 tinyvm 解释执行；不在设备上生成可执行原生代码，也不把下载模块编译或装载为原生动态代码。

这里排除的 AOT 特指“设备端把下载模块预编译成可执行原生代码”。开发或发布阶段把源码编译为标准 `.wasm`，以及应用本身的构建期 AOT，都不在此排除范围。`agenterm-dyn` 的本地原生动态调用面也不是 iOS tinyvm 执行路径。槽 B 暂停，直到目标分发模型与平台权限发生可验证的变化。

tinyvm 已经按事实上的 WebAssembly VM 建设，而不是只够运行现有游戏的专用解释器。
标准 `.wasm` 是长期执行格式；TinyArcade v1 只冻结当前可接受的能力 profile 和 host ABI，
不冻结 VM 的标准能力上限。标准 proposal 按解码、验证、执行、预算、独立引擎差分证据
逐项进入，平台专有需求只能通过版本化 standard imports 扩展，不能发明私有 guest opcode。
这是跨平台可扩展应用的底层选择，也为未来非游戏宿主保留同一 VM 内核。

tinyvm 的产品身份是自有、跨平台、可预算的标准 WebAssembly VM，不是 H5 小游戏、
JavaScript miniApp、WKWebView 容器，也不再由早期 compact bytecode 实验定义。TinyArcade
是该 VM 之上的第一个版本化 host platform；其它可扩展应用同样可以使用标准 `.wasm`
和自己的窄 host ABI。上层通过经预算的 native imports 提供渲染、输入、音频、时钟
与存储面，不依赖 DOM、JavaScript 或 Web 容器语义。App Store 可接受性是 TinyArcade
分发层的独立发布门，不反向改写 tinyvm 的通用运行时架构。

JavaScriptCore 内部存在 WebAssembly 实现，但 Apple 公开的嵌入面是 `JSContext` 中的 JavaScript 执行，不是独立的原生 WASM module/instance API。JSC 可以作为后续对照基准或实验后端，但 tinyvm 是权威、可移植、可预算的 baseline；任何游戏都不得依赖 JSC 才能运行。

开发期对照已经成为可执行回归门：同一个标准 `.wasm`、同一份 TAR1 输入/时钟、同一
portable snapshot 与 host RNG，分别交给 tinyvm 和系统 JavaScriptCore WebAssembly
执行，并逐帧比较 render/audio 的精确长度与 SHA-256。Depth Well 与 Paddle Guard
同时覆盖 grid3d、indexed2d 和 tones。该 adapter/runner 只位于 macOS 测试目录，
不链接 iOS package、不进入 nostalgia-arcade，也不构成 H5 小游戏平台。
公开 API、WebKit 内部能力、实验矩阵和 App Review 风险分层见
[`tinyarcade-javascriptcore-boundary.md`](../docs/tinyarcade-javascriptcore-boundary.md)。

完整的 iOS 游戏运行底层验收树与依赖路径见 [`plan/goal-tinyvm-ios-game-runtime.md`](../plan/goal-tinyvm-ios-game-runtime.md)。

游戏卡带坚持使用标准 `.wasm` module；不增加 tinyvm 私有 opcode，也不把执行体改成私有二进制格式。核心能力由版本化的标准 function import namespace `tinyarcade:core/v1` 提供。Native 模块同样使用独立版本化 namespace，并且只有宿主 capability registry 明确注册的精确签名才能绑定；未知能力默认拒绝。C ABI v1.8 与 Swift package 已能为 bundled/reviewed 卡带注册这些能力，在调用 app callback 前执行每生命周期配额，并读取上一生命周期的确定性 execution stats；private-user 卡带保持 core-only。这样编译器、转换器与粉丝自制工具只需遵循卡带 ABI，而不依赖 tinyvm 内部实现。

官方远端目录和用户私有导入是两条不同的产品/审核路径：私有导入只进入用户自己的 app library，不自动公开或分发给其他用户；官方目录才走签名、复核、撤销与兼容性门。两条路径共同执行 WASM 验证、资源预算和 capability negotiation。

2026-06-08 版 Apple Guidelines 只明确列出 HTML5/JavaScript mini games 等类别，
自定义 WASM 语言没有自动取得 4.7 例外，4.7.2 对 native API 暴露还要求事先批准。
因此 Swift App 面默认 `appStoreBundledOnly`：private/reviewed runtime 和 library 在
任何 I/O/guest work 前拒绝。只有显式 `appleApprovedExternalCartridges` policy 与有界
approval reference 才能解锁；reference 是 release audit 声明，不是假装获得许可。
SDK 测试 policy 不公开，首版可保持固定 bundled games 且不暴露下载/导入 UI。

卡带兼容性以“标准 Wasm 文件 + 版本化平台契约”为准，而不是以某一版 tinyvm
内部实现为准。manifest 放在标准 custom section；core/native 能力都只表现为标准
function import，namespace、函数名、值签名和版本必须精确匹配。未来转换器可以只读
manifest/import table 就生成兼容性报告；未知 native module 默认拒绝，绝不把声明
本身视为装载原生代码的授权。`tinyarcade:core/v1` 的语义冻结，新增 native 能力使用
独立 `authority:module/vN`，不得暗改旧版本。

未来 native module 是随审核版 App 编译进去的宿主实现，不是卡带携带的 dylib、AOT
产物或私有 opcode。粉丝转换器以显式 host profile 为目标，输出仍是标准 `.wasm`，并
根据标准 import table 生成所需 core/media/native 版本与资源上限的机器可读兼容报告。
粉丝“上传给自己玩”只进入个人 private-user 安装链，即使以后经个人账号或私有云传输，
也不会因此获得公开目录或 official-reviewed 身份；对外部代码的 Apple 许可门仍然生效。

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

App build 的可用能力也不再靠说明文字猜测。TAH1 host profile 确定性记录 core/media
版本、WASM 与输出资源上限，以及已经编译进 App 的 native module 精确
namespace/field/i32 signature/每生命周期调用配额。Rust、CLI、C ABI v1.8 与 Swift
共用同一 encoder 和非执行兼容检查；转换器可在上传前拒绝缺失或签名不匹配的 import，
同时仍须另跑 fuel、媒体输出与 native 语义的动态 conformance。

离线 publisher 现在必须接收该 TAH1，并在签名前用它静态检查每枚卡带；输出目录固定
携带 `host-profile-v1.tahost` 及 catalog 根级 length/SHA-256。网站和转换器可据此选择
精确 App build，但 App 不信任 catalog 自报的 profile 权限：Swift 只在受限同源下载
结果与本地编译配置生成的 TAH1 逐字节相等时接受，因此目录无法自行扩大 native import
或资源上限。旧 catalog 可不带该发现字段，仍保持只读兼容。

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

C ABI v1.8 保留并验证 Rust trust/cache 与 iOS App 之间的完整边界。独立的单线程 cache
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

运行预算现在也不是只靠配置上限和 wall-clock 日志猜测。persistent Wasm instance
记录上一 top-level invocation 的实际指令数、当前 memory pages 与 table elements；
GameRuntime 再绑定 lifecycle、native dispatch、render/audio/state bytes。C ABI v1.8
和 Swift 可在成功或 guest trap 后读取同一 allocation-free record，host input 在执行前
被拒绝则不会篡改它。两个真实卡带在 booted iPhone 17 Pro simulator 的 600 帧
Release 运行中，Depth Well 峰值 13,150 steps/17 pages，Paddle Guard 峰值 37,864
steps/17 pages；逐帧统计与输出长度和配置上限一致。wall time/thermal/process memory
仍属于设备证据，不伪装成跨平台确定性数据。

第二枚生产证明卡带 Paddle Guard 已消除“运行时只是为 Depth Well 特制”的可能：
它是 5,280-byte 严格 WASM MVP module，只导入八个 `tinyarcade:core/v1`
function，用 160×120 indexed frame、通用 input bits、impact/success/failure
tones 与 64-byte guest state 完成另一种街机循环。Rust 黑盒覆盖发射、移动、
护盾反弹、漏球、清场升级和逐字节恢复；iOS smoke 覆盖完整原生呈现。物理设备
仍未连接，因此 real-game proofs 和 native I/O surface 继续保持 partial。
