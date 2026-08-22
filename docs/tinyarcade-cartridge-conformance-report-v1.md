# TinyArcade cartridge conformance report v1

Status: implemented converter/CI exchange schema

`tinyvm cartridge check GAME.wasm --json` emits one compact UTF-8 JSON object
followed by a newline. Unlike the static host-profile report, this command
instantiates the private/core-only cartridge and executes the bounded init,
first tick, suspend, second tick, fresh-instance init, resume and replay tick
sequence. It validates render/audio records and requires the expected and
restored render/audio bytes to match exactly.

The existing human-readable output remains the default. With `--json`, stdout
contains only the report and stderr remains empty for all reportable outcomes:

```text
exit 0  static_valid=true, dynamic_valid=true, deterministic=true
exit 1  input/static/dynamic/media/determinism rejection
```

An invocation/usage error is not a report and continues to print usage on
stderr.

## Schema tree

```text
tinyarcade-cartridge-conformance-report
├── schema                                      fixed string
├── schema_version                             integer, 1
├── valid                                      boolean; complete gate result
├── static_valid                               boolean
├── dynamic_valid                              boolean
├── deterministic                              boolean | null
├── cartridge                                  object | null
│   ├── game_id / game_version                 string
│   ├── abi_version / state_version            integer
│   ├── wasm_bytes                             integer
│   ├── native_capabilities                    string[]
│   ├── wasm_features                          string[]
│   └── function_imports                       typed import object[]
├── limits                                     object
│   ├── max_table_elems / max_memory_pages     integer
│   ├── max_steps                              integer
│   ├── max_call_depth / max_activation_slots  integer
│   └── max_render/audio/state_bytes           integer
├── evidence                                   object | null
│   ├── render_stream                          schema string
│   ├── initial_render/audio_bytes             integer
│   ├── application_metadata_schema            integer | null
│   ├── application_metadata_bytes             integer
│   ├── snapshot_bytes                         integer
│   ├── expected_render/audio_bytes            integer
│   ├── replay_render/audio_bytes              integer
│   └── lifecycle_stats
│       ├── initial_init / initial_tick
│       ├── suspend / expected_tick
│       └── restored_init / resume / replay_tick
└── error                                      object | null
    ├── stage                                  fixed stage string
    └── message                                non-empty escaped string
```

Each lifecycle-stat object carries `wasm_steps`, `peak_call_depth`,
`peak_activation_slots`, `memory_pages`, `table_elements`, `native_calls`,
`render_bytes`, `audio_bytes` and `state_bytes`. These are deterministic VM/ABI
facts, not wall-time measurements.

Failure stages are stable schema values: `input`, `static_validation`,
`initialization`, `initial_tick`, `initial_media`, `suspend`, `expected_tick`,
`expected_media`, `restore_initialization`, `resume`, `replay_tick`,
`replay_media` and `determinism`. `deterministic` is `false` only when both
paths ran but their bytes differed; it is `null` when execution stopped before
that claim could be evaluated. A statically valid cartridge retains its
descriptor in a dynamic failure report; unreadable or statically invalid input
uses `cartridge=null`.

The same structured execution function is used by the catalog publisher, so a
publisher cannot silently apply a weaker dynamic gate than the converter CLI.
The report includes no file path, timestamp or native callback and uses the
same complete JSON escaping and deterministic ordering as the static
host-compatibility report. Identical cartridge bytes therefore produce
byte-identical reports.

This command deliberately grants no native module. Run the separate
`check-profile ... --json` static report to compare requested native imports and
standard feature families with one exact app build. Passing either report alone
does not establish the other claim, catalog trust, Apple distribution approval
or physical-device performance.
