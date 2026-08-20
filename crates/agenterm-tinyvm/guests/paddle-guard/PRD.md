# Paddle Guard product record

```text
Paddle Guard
├── product promise
│   ├── understand paddle, spark and panels from one screen
│   ├── make a useful left/right decision immediately
│   ├── reward controlled rebounds rather than random spectacle
│   └── feel like an original early-arcade cartridge without copied assets
├── core loop
│   ├── move the shield below a docked spark
│   ├── press primary to launch
│   ├── return the spark and change its angle with paddle contact
│   ├── break the five-by-eight panel field
│   ├── preserve three lives across misses
│   └── clear the field to increase speed and level
├── simplicity constraints
│   ├── left / right / primary only             [x]
│   ├── one screen and no scrolling              [x]
│   ├── no power-ups, currencies or metagame     [x]
│   ├── no external images, fonts or sounds      [x]
│   └── deterministic fixed-point physics        [x]
├── native-platform proof
│   ├── standard WASM MVP module                 [x]
│   ├── core/v1 imports only                     [x]
│   ├── indexed2d/v1 complete frame              [x]
│   ├── generic impact/success/failure tones     [x]
│   ├── bounded portable suspend state           [x]
│   └── UIKit native presentation                [x]
└── acceptance
    ├── converter accepts the published bytes    [x]
    ├── launch, move, rebound and loss tested     [x]
    ├── suspend/resume replay is byte-identical   [x]
    ├── iOS device/simulator package links        [x]
    ├── booted-simulator playable-frame run       [x]
    └── physical-device play session              [ ]
```

The cartridge is an independent runtime acceptance game, not a clone of a
named commercial title. Geometry, palette, tiny digit glyphs, rules and tones
are generated from original code. Its job is to prove that a second genre can
use the same standard cartridge contract without a Depth Well-specific host.
