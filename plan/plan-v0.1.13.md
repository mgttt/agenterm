# AgenTerm v0.1.13 公开计划

状态：布点草案（2026-08-02）；不改变 v0.1.12 发布状态，不触发 Candidate、tag 或 Release。

主题：**把平台 crate 的单一事实、错误语义与可复用 facade 做得更窄、更稳定**。

## 目标树

```text
平台抽象收敛
├─ [ ] 路径/目录失败保持 typed Failed/Unsupported，禁止静默 temp fallback
├─ [ ] Control Center 截图策略由 agenterm-platform 单一提供
├─ [ ] CapabilityStatus / PlatformSnapshot 减少主 crate 重复映射
├─ [ ] 薄包装 facade 审计：删除纯转发，保留产品策略 glue
├─ [ ] 外部依赖 feature bundle 与最小依赖树回归
└─ [ ] 统一跨平台 fixture/nonce/RAII cleanup，降低并行测试碰撞
```

## 依赖顺序与证据

1. 先冻结现有 API 与失败码，审计 `src/platform/services/paths.rs`、
   Control Center screenshot、Capability JSON 的调用者。
2. 先修错误语义，再合并策略 facade；避免在 fallback 仍存在时抽象 API。
3. 以 `cargo tree --no-default-features`、feature matrix、boundary tests、
   unit tests 和 CLI/Control Center smoke 证明没有依赖膨胀或产品行为漂移。
4. 最终串行执行 fmt、Clippy、crate tests、Agenterm quick/check；发布动作另行授权。

## 设计约束

- 平台原生选择只在 `agenterm-platform` 的 `selected.rs` / adapters；主 crate
  只保留 Agenterm 命名、workspace/instance policy 和产品 renderer glue。
- `Unsupported` / `Failed` 必须可观察；不能把权限、路径、解析或 native 失败改写成
  临时目录、默认平台或“可用”。
- 公共 contract 不泄漏 Win32/POSIX/第三方原生句柄；策略通过 caller-owned trait 接入。

## 明确非目标

- 不在本版本扩展 net、WebView、Fleet 或 Control Center 产品功能。
- 不重做已经完成的 PTY、IPC、输入、窗口和 Script Runtime 迁移。
- 不创建 tag/Candidate/Release；发布仍需独立 exact-SHA 授权链。
