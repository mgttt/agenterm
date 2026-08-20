# Paddle Guard cartridge

Paddle Guard is the first complete indexed-2D TinyArcade cartridge. It uses
only original procedural pixels and tones and imports only
`tinyarcade:core/v1` from an ordinary standard `.wasm` module.

Build and validate from the repository root:

```sh
crates/agenterm-tinyvm/build-paddle-guard-cartridge.sh
cargo run -p agenterm-tinyvm --bin tinyvm -- \
  cartridge check target/tinyvm-paddle-guard/paddle-guard-0.1.0.wasm
```

Input mapping:

```text
left/right       move the shield
primary          launch or restart after game over
menu             host-owned pause/back; never delivered as a game action
```

The guest owns fixed-point physics, panels, score, lives, level and portable
state. The native app owns layout, controller mapping, pause UI, synthesis and
safe persistence. The full product and acceptance tree is in `PRD.md`.
