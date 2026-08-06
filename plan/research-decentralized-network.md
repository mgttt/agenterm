# 去中心化网络选型调研（为 CC 未来需求）

状态：**调研，未动代码**  
时间：2026-08-06  
背景：CC（Control Center）未来有需求要跑在去中心化网络上。本文只做选型与风险交底，
**不预设一定要做**；真要落地前请先看 §5 的决策项。

## 1. 结论先说

| 选项 | 判断 |
|---|---|
| **iroh 1.0** | **推荐**。2026-06-15 发 1.0，wire protocol 稳定，按公钥拨号 |
| rust-libp2p | 可用但 **0.56.x，明确未稳定 API**，每次小版本都可能破坏 |
| Rust 版 IPFS | **不推荐**。没有健康的可嵌入实现 |
| kubo daemon | 要额外带一个 Go 进程，跟我们刚做的「减可执行文件」相反 |

**为什么是 iroh 而不是 libp2p**：我们的场景是「连我自己的另外几个实例」，
iroh 的按公钥拨号正好是这个模型；libp2p 的 Kademlia DHT 是为「在全球 DHT 里
发现陌生人」设计的，对我们是杀鸡用牛刀，还多一个 bootstrap 节点依赖。
而且 iroh 把 NAT 打洞做进了 QUIC 握手本身，不是 relay + DCUtR 那套外挂组合，
活动部件少。`iroh-blobs`（内容寻址，BLAKE3）/ `iroh-gossip`（pubsub）都是一方
crate，本来就是配套设计的。

**为什么不碰 IPFS**：`beetle` 已经事实停摆（维护者自己把可嵌入性推到「不确定的
将来」）；`rust-ipfs` 是社区 fork，不是 Protocol Labs 背书；`ipfs-embed` 查不到
2025/2026 的维护活动。要内容寻址的话，直接用 `iroh-blobs` 或自己按 BLAKE3/CID
约定做，比引一个半死的 IPFS crate 强。

## 2. 成熟度事实

- **iroh 1.0**（2026-06-15，N0 Inc.）：4 年 65 个预发布之后的 1.0。承诺 wire
  protocol 稳定 —— 任意两个 v1 endpoint 跨小版本、跨语言可互通；Python/Node/
  Swift/Kotlin binding 同时稳定。**注意**：0.35 那条老线的公共 relay 只支持到
  2026-12-31，要做就直接上 1.0 API。
- **rust-libp2p 0.56.x**：项目自己的 [#3072](https://github.com/libp2p/rust-libp2p/discussions/3072)
  就在跟踪「什么时候能 API 稳定」，`NetworkBehaviour`/`Swarm` 内部仍在破坏性变更。
  其中 QUIC / TCP / mDNS / Gossipsub 算生产级；Relay v2 + DCUtR 能用，但
  **本质上依赖第三方 relay 基础设施**，这是最脆的一环（是运维依赖，不是技术问题）。

## 3. 体积

我们刚把 mux/mcp 并进 CLI 省了 1.6 MB，所以这条要认真对待。

调研给的是**源码体积**不是编译后贡献，不能直接用。可确定的是：iroh 依赖谱系较窄
（quinn / rustls 0.23 / tokio / ring），且它自己有明显的 feature flag 纪律；
libp2p 是一堆 sub-crate 的门面，实际重量取决于开了哪些 behaviour，历史上
[#1051](https://github.com/libp2p/rust-libp2p/issues/1051) 就有体积抱怨。

**真要选型前必须自己量**：开一个只带 QUIC+mDNS+gossip 的最小构建，
`cargo bloat --release --crates` 前后对比。别信源码体积。

## 4. 威胁模型（这条最重要）

AgenTerm 是**终端 / agent 工具**，本来就跑命令、读文件，信任级别已经很高。
再加一个**监听网络的组件**，任何实现 bug 的代价都比普通 app 大得多。

必须守的设计底线：

1. **编译期 feature gate**：不开这个 feature 的构建，零额外体积、零监听端口。
2. **运行时默认关**：用户不显式开启，就不开 socket、不发现广播。
3. **默认只走 LAN（mDNS + 直连）**，跨网 relay 是**另一个**更显式的开关。
   这两件事风险完全不同，不能混在一个开关里。
4. **应用层配对授权**：传输加密（libp2p Noise / iroh 公钥）不等于授权。
   要有显式配对（配对码 / 手动交换公钥），不能「局域网里能连上的都算自己人」。
5. **绝不默认加入任何公共网络**：不 bootstrap 公共 IPFS DHT，不进公共 gossip topic。
6. **可见的开关和状态指示**：用户随时能看到「P2P 正在监听 / 正在经 relay」。

**relay 这件事要说清楚**：不管 libp2p 还是 iroh，打洞失败时（对称 NAT、企业防火墙）
都要回落到 relay。这意味着流量元数据会经过一个你不控制的第三方，可用性也绑在
对方的 uptime 上。这是 NAT 穿透的通性（WebRTC 的 STUN/TURN 一样），不是哪个库的
缺陷，但**是我们要替用户承担的运维依赖**，得写进产品说明而不是藏起来。

## 5. 决策项（交给你拍板，我不自己定）

1. **要不要做**。现在 CC 产品设计还没定，这块可以先不动。我倾向于**等 CC 产品形态
   清楚了再选型**，否则很容易先绑一个网络栈再倒推需求。
2. **如果做，先做哪一半**。「LAN-only mDNS 直连」和「跨网 relay」是两个产品，
   前者无外部依赖、可完全离线，后者要么自建 relay 要么依赖 N0。建议**先只做前者**。
3. **要不要自建 relay**。用 N0 公共 relay 上手快，但把可用性和元数据交给第三方。

## 6. 待验证（我没查实，别当结论用）

- iroh 的本地直连能否**完全不碰 N0 基础设施**（真·离线局域网模式）。架构上看
  合理，但没找到明确确认，落地前要看源码验证。
- 2026 年真实 NAT 组合下 DCUtR 打洞成功率，没找到权威数字。
- libp2p 0.56.0 之后是否有更新的 patch 版本。
- 两者编译后的实际 MB 增量 —— 见 §3，必须自己量。

## 参考

- [Iroh 1.0 blog](https://www.iroh.computer/blog/v1) ·
  [n0-computer/iroh](https://github.com/n0-computer/iroh)
- [rust-libp2p](https://github.com/libp2p/rust-libp2p) ·
  [API stability #3072](https://github.com/libp2p/rust-libp2p/discussions/3072)
- [beetle 可嵌入性停摆 #88](https://github.com/n0-computer/beetle/issues/88) ·
  [rust-ipfs](https://github.com/dariusc93/rust-ipfs)
