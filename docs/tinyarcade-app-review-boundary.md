# TinyArcade App Review boundary

Policy snapshot: 2026-08-21, based on Apple's App Review Guidelines last
updated 2026-06-08. This is an engineering release gate, not a claim that Apple
has pre-approved a custom WASM marketplace.

Apple guideline 2.5.2 requires App Store apps to be self-contained and says
they may not download, install or execute code that introduces or changes app
features. Guideline 4.7 permits particular non-bundled software categories,
explicitly including HTML5/JavaScript mini apps and games, streaming games,
chatbots, plug-ins, and downloaded games for retro console/PC emulators. A
custom TinyArcade WASM game platform is not expressly named. Apple's Mini Apps
Partner Program separately describes another language only when approved by
Apple.

Authoritative sources:

- <https://developer.apple.com/app-store/review/guidelines/>
- <https://developer.apple.com/programs/mini-apps-partner/>

The first App Store release therefore uses this boundary:

```text
submission-safe baseline
├── Depth Well cartridge embedded in the signed app bundle
├── app remains useful and fully playable with no download
├── no remote .wasm execution
├── no Files/private-cartridge execution
├── no public creator upload or marketplace UI
├── no claim that TinyArcade is a retro console emulator
└── Review Notes disclose the bundled interpreter and fixed game purpose
```

The technical SDK may continue developing signed-catalog and private-import
paths, but the App Store product target must not expose or call them until Apple
has explicitly clarified or permitted this custom language/use case. TestFlight
is not an exemption: guideline 2.2 says TestFlight apps intended for public
distribution should comply with the review guidelines.

If Apple permits the external-cartridge mode, release still requires the full
4.7 surface rather than only a signature:

```text
permissioned external mode
├── all offered software remains developer responsibility
├── objectionable-content filter/report/response/block mechanisms
├── StoreKit-compliant digital goods
├── no exposed native platform API without prior permission
├── per-game explicit consent before sharing data/privacy permission
├── complete software index and universal link for every offered game
├── per-game age metadata and underage restriction
└── accurate App Store metadata and detailed Review Notes
```

The SDK's `tinyarcade://game/<game-id>` form is presently an internal
selection-only contract, not the universal-link evidence required by 4.7. It
rejects query/fragment launch flags and cannot download or execute. A shipping
external mode would additionally need one public HTTPS universal link per game,
with the moderation, age and commerce metadata above, after Apple permission.

Private local import remains technically safer than public distribution, but it
still executes code outside the submitted bundle and therefore is not treated
as automatically allowed under 2.5.2. It stays disabled in the baseline until
Apple answers that exact question.
