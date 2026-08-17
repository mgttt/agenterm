# PRD 02.35 — agenterm-tinyvm

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped · `[~]` partial · `[ ]` planned

```
agenterm-tinyvm (35)
├── eval(bytes)          [x]
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
    ├── dyn              [x]
    ├── chassis          [x]
    ├── #78              [x]
    ├── WASI             [x]
    ├── APE              [x]
    └── WAT              [x]
```

`eval(bytes)` → 值或错。程序是标准 `.wasm`。
宿主门是 import 表。未绑定即 trap。
槽 A = WASM 1.0 MVP 172 操作码。双绿。
核 strip `<100KiB`。槽 B（dyn/AOT）停。
