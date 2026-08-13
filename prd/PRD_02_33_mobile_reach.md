# Mobile reach (`agenterm-mobile`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

This module is the root of the **mobile reach** product. It owns how a person
away from the desk attaches to an existing desktop `agenterm server` — first
as a **Progressive Web App** on the public site, later as optional store
apps. It does not own the desktop workbench, `agenterm-con`, computer-use,
or the distribution/install bytes of `agenterm.work`.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

Every requirement below is `[ ]` planned. Opening this module is not a
version commitment and does not start iOS or Android engineering.

## Why this product exists

- [ ] AgenTerm's daily value is a long-lived **desktop** fleet. People still
  leave the desk. They need a comfortable phone surface to see that fleet and
  later to pair with it — not a second terminal OS on iOS/Android.
- [ ] Apple App Store review is slow; a native iOS app will not be started
  soon. A native Android app is likewise deferred. Those store shells stay
  **placeholders** on this tree so they cannot be forgotten or reinvented
  inside another module.
- [ ] Until a store app exists, the phone surface is a **PWA** shipped from
  the same `docs/` site that already is the public homepage, at
  **https://agenterm.work/**.

## Subtree map

Children are not split out yet. When pairing or a store shell grows past
this file, they become 34+. Until then the three branches live here:

```text
agenterm-mobile (33)
├── PWA @ https://agenterm.work/app     ← first host; reuse docs/
├── store apps (iOS / Android)         ← placeholder only
└── device pairing / remote collab     ← QR bind to desktop client
```

Native protocol/client-core sequencing remains an execution projection in
[`plan/plan-mobile.md`](../plan/plan-mobile.md) (M-A / M-B / M-C). That
plan does not own product status.

## Product outcome

- [ ] a person on a phone can open https://agenterm.work/, tap **Mobile
  App**, and get an installable PWA that is clearly a **connector** to a
  desktop AgenTerm — never a standalone mobile PTY fleet.
- [ ] later, the same PWA (and, much later, store apps) can **scan a QR
  code** shown by the desktop client, bind as a named device, and do
  remote collaboration under an explicit grant (observe first).
- [ ] iOS and Android store listings remain reserved names and empty
  shells in this contract until a human authorizes native work. Review
  latency is an accepted reason not to start them.

## Positioning (frozen)

- [ ] the phone is the **third host**: it attaches to an existing
  `agenterm server`. It does not run agent/terminal authority, does not
  start a server, does not own workspace persistence, and does not grow a
  second tab/PTY tree.
- [ ] this matches inspiration Lane F and rejects F6 (full mobile PTY
  fleet). See
  [19](PRD_02_19_inspiration_and_future_vision.md).
- [ ] one public contract, many clients: GUI, CLI, mux, script, MCP, PWA,
  and future store apps all consume the same control-plane truth
  ([07](PRD_02_07_agent_control_plane.md)). The PWA must not invent a
  second snapshot/wait/receipt dialect.
- [ ] `validate_local()` loopback/IPC rules stay a **desktop** safety
  boundary. Mobile reach is a **new** authorization face. Pairing must
  not punch a hole in local IPC.

## PWA (first host)

- [ ] **Origin.** Canonical public origin is `https://agenterm.work/`.
  Site source is [`docs/`](../docs/). Current Pages CNAME
  `agenterm.mega.tech` is a transitional alias until the
  [18 M13](PRD_02_18_roadmap.md) cutover; the PWA must not become a
  second site or a second CNAME.
- [ ] **Entry.** The existing homepage gains one obvious **Mobile App**
  navigation target. That target opens the PWA shell at
  `https://agenterm.work/app` (path may be `/mobile` only if `/app` is
  taken by something else; pick one and keep it). `/` remains docs +
  install; `/app` is the product UI.
- [ ] **Install.** A Web App Manifest + service worker make the shell
  installable on iOS Safari and Android Chrome as a standalone display.
  Offline: the chrome and last honest pairing state may cache; live
  fleet data must not pretend to be current when the desktop is gone.
- [ ] **First visible slice (no pairing yet):** identity of the product,
  “this talks to your desktop AgenTerm”, a disabled or empty paired-
  devices list, and a documented “scan QR when the desktop shows one”
  placeholder. Comfortable to open every day even before bind works.
- [ ] **No store wrapper.** The PWA is not a fake App Store binary and
  does not wait on Apple/Google review.
- [ ] **No extra JS product stack in the desktop binary.** The PWA lives
  under `docs/` (static + small client). It must not drag npm into the
  Rust workbench or the 0.1.18 App Pack gate.

M13 still owns install.sh, `releases.json`, and provenance on the same
host. This module owns only the Mobile App entry and `/app` behavior.

## Store apps (placeholders)

- [ ] **iOS** (`agenterm` on the App Store): reserved. Not scheduled.
  Reason: official review is too slow to be the first phone surface.
- [ ] **Android** (Play): reserved on the same timeline — not because
  Play is the same bottleneck, but because two native shells before a
  working PWA is the wrong order.
- [ ] when a store app is authorized, it is a **shell** over the same
  client-core as [`plan/plan-mobile.md`](../plan/plan-mobile.md) M-B/M-C
  (Flutter / RN / Tauri 2 still undecided — K1). It must speak the same
  pairing and grant model as the PWA. It must not fork a third protocol.
- [ ] store apps are never a prerequisite for the PWA. The PWA is never
  deleted just because a store binary appears.

## Device pairing and remote collaboration

- [ ] **QR bind.** The desktop client can show a short-lived pairing
  invite as a QR code (and an equivalent URL). The phone camera or PWA
  file-picker scans it. Success binds a device name to that desktop
  identity.
- [ ] **Invite properties:** explicit scope, expiry, nonce, and the
  desktop's server-scope identity. Replay and wrong-peer must fail typed.
  Research pairing in [22](PRD_02_22_decentralized_network.md) may inform
  the crypto; this module owns the **user-visible** bind/unbind UX.
- [ ] **Grant ladder:** a new device starts at **observe** (tree +
  bounded summaries). Composer / input / destructive actions need a
  distinct explicit grant, revocable from the desktop. Possession of the
  QR is not lasting authority.
- [ ] **LAN first.** Binding on the same network is the first real
  increment. Off-LAN remote collaboration is a later increment and needs
  its own threat model; it must not ship by relaxing loopback IPC.
- [ ] **cu / net are not blockers.** Pairing does not wait for
  `agenterm-cu` window-place or for `agenterm-net` to leave research.
  If net later carries the bytes, the invite/grant UX stays here.

Inspiration F1–F4 (connect, monitor, mobile Composer, push) promote into
this module when they get evidence. They stay `[ ]` until then.

## Explicit non-goals

- [ ] no mobile PTY, no on-phone `agenterm server`, no workspace
  authority on the device (F6 remains rejected).
- [ ] no silent always-on remote access; no pairing that survives
  revoke; no credentials in QR payload, logs, or push text.
- [ ] no starting iOS/Android Xcode/Android Studio work from this PRD
  text alone.
- [ ] no second public website, and no hijacking `/` away from docs +
  install.
- [ ] no borrowing a workbench milestone (including v0.1.18 / v0.1.19)
  to claim the PWA or a store app shipped.

## Version gate

- [ ] **no version is assigned.** Roadmap ownership is
  [18](PRD_02_18_roadmap.md). Implementation is slow and incremental.
- [ ] suggested order, each its own later execution leaf:
  1. `docs/` nav + `/app` static shell (honest placeholder UI);
  2. Web App Manifest / installability;
  3. desktop QR invite + PWA bind (LAN, observe-only);
  4. observe live tree / bounded summaries;
  5. Composer grant;
  6. store apps, only after a human decision.
- [ ] a slice ships only with public black-box evidence against the real
  origin (or a documented preview host) and a real desktop. Design text
  is not evidence.

## Boundary

| Surface | Owner | Relation |
|---------|--------|----------|
| Homepage, install, `releases.json` | [17](PRD_02_17_delivery_quality.md) / M13 | same origin; this module only adds Mobile App → `/app` |
| Typed ops, snapshot, waits | [07](PRD_02_07_agent_control_plane.md) | consumed, not forked |
| Human workspace / Composer | [06](PRD_02_06_human_workspace.md) | phone Composer is a client of 06 |
| Decentralized transport | [22](PRD_02_22_decentralized_network.md) | optional later carrier |
| Native iOS/Android shell engineering | [`plan/plan-mobile.md`](../plan/plan-mobile.md) | execution only; status lives here |
