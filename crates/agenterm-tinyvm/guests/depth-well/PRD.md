# Depth Well product record

```text
Depth Well
├── product promise
│   ├── understand the objective from one falling piece
│   ├── make useful placement decisions within seconds
│   ├── preserve the spatial pleasure of a classic 3D block well
│   └── use modern readability without adding progression clutter
├── core loop
│   ├── inspect active piece and landing ghost
│   ├── translate on the 5 × 5 horizontal plane
│   ├── rotate around any of three axes
│   ├── let gravity fall or commit with hard drop
│   ├── lock four cubes into the 10-deck well
│   └── clear a completely filled 5 × 5 deck
├── simplicity constraints
│   ├── one board, one active piece, one score
│   ├── no hold slot                         [x]
│   ├── no power-ups                         [x]
│   ├── no currencies or metagame            [x]
│   ├── five-piece fair shuffle bag          [x]
│   └── one-cell wall kick                    [x]
├── feedback
│   ├── settled / ghost / active cell roles  [x]
│   ├── score, decks, level, game-over flag  [x]
│   ├── lock / clear / game-over tone cues   [x]
│   └── native camera and touch design        [ ]
├── deterministic runtime
│   ├── host clock and seeded RNG only        [x]
│   ├── bounded grid3d and tone streams       [x]
│   ├── versioned portable state              [x]
│   ├── replay after suspend is byte-identical [x]
│   └── no network/native capability imports  [x]
└── acceptance
    ├── strict standard WASM MVP artifact      [x]
    ├── tinyvm black-box load and play         [x]
    ├── 17-page memory ceiling                 [x]
    ├── 100,000-instruction call ceiling       [x]
    ├── cartridge below 16 KiB                 [x]
    ├── iOS renderer/controller integration    [ ]
    └── physical-device play session           [ ]
```

The initial piece set deliberately favours planar four-cube silhouettes. The
third rotation axis makes them spatial without asking a first-time player to
decode exotic polycubes immediately. More pieces are a later tuning decision,
not an automatic content expansion.
