# TinyArcade replay conformance report v1

Status: implemented creator-tool/CI exchange schema

`tinyvm replay check GAME.wasm TRACE.tareplay --json` emits one compact UTF-8
JSON object followed by a newline. It decodes the bounded TAR1 trace, verifies
its complete cartridge hash and embedded identity, creates the private
core-only runtime, restores the recorded initial snapshot, then regenerates and
checks every render/audio length and SHA-256.

The report complements two separate claims:

- `cartridge check ... --json` proves the fixed init/tick/suspend/resume probe.
- `cartridge check-profile ... --json` proves static compatibility with one
  exact app-build host profile.

A representative replay covers author-selected gameplay inputs, but does not
replace either claim. The text command remains unchanged. JSON reportable
outcomes use stdout only and preserve process status:

```text
exit 0  valid=true, replay_valid=true, cartridge_bound=true
exit 1  input/decode/binding/initialization/execution rejection
```

An invocation/usage error is not a report and continues to print usage on
stderr.

## Schema tree

```text
tinyarcade-replay-conformance-report
├── schema                                      fixed string
├── schema_version                              integer, 1
├── valid                                       boolean; complete replay result
├── replay_valid                                boolean; canonical TAR1 decoded
├── cartridge_bound                             boolean | null
├── identity                                    object | null
│   ├── game_id / game_version                  string
│   └── abi_version / state_version             integer
├── cartridge                                   object | null
│   ├── bytes                                   integer
│   └── sha256                                  64 lowercase hex characters
├── trace                                       object | null
│   ├── bytes                                   integer
│   ├── sha256                                  64 lowercase hex characters
│   ├── initial_snapshot_bytes                  integer | null
│   └── steps                                   integer | null
├── limits                                      object
│   ├── max_table_elems / max_memory_pages      integer
│   ├── max_steps                               integer
│   ├── max_call_depth / max_activation_slots   integer
│   └── max_render/audio/state_bytes            integer
├── evidence                                    object | null
│   ├── verified_frames                         integer
│   ├── total_render_bytes                      integer
│   ├── total_audio_bytes                       integer
│   ├── first_clock_ms                          integer | null
│   └── final_clock_ms                          integer | null
└── error                                       object | null
    ├── stage                                   fixed stage string
    └── message                                 non-empty escaped string
```

Failure stages are stable schema values: `cartridge_input`, `replay_input`,
`replay_decode`, `replay_coverage`, `cartridge_binding`, `initialization` and
`replay_execution`. `replay_coverage` rejects a canonical but zero-frame trace;
representative evidence must execute at least one input/clock step.
`replay_valid=true` means only that the complete trace
decoded under TAR1 bounds. `cartridge_bound=false` means an exact hash or
manifest identity mismatch was observed; it is `null` before binding could be
evaluated. `evidence` is present only after every frame matched, so a partial
prefix is never presented as successful coverage.

`limits` repeats the effective core-only converter ceilings used to construct
the runtime: aggregate table elements, memory pages, instructions per lifecycle
call, call/activation depth and render/audio/state bytes. The report therefore
does not rely on an unstated local CLI default when archived as CI evidence.

The two artifact objects expose content facts, never source paths. A malformed
but readable trace retains its byte length and digest while its decoded fields
remain `null`; an unreadable artifact is `null`. There is no wall clock,
filesystem path or native callback in the report. Identical cartridge and
trace bytes therefore produce byte-identical JSON.

This command deliberately grants no native module and does not sign, publish or
approve a game. It proves deterministic execution only for the recorded input
and clock sequence. Creator CI should keep meaningful traces for core gameplay
routes and every fixed regression, then use the development JSC/H5 oracle when
cross-engine evidence is required.

The offline catalog publisher requires one passing representative trace per
source game before signing. It calls the same structured checker rather than a
publisher-only replay implementation, and never copies the review trace into
the runtime catalog output.
