# TinyArcade host compatibility report v1

Status: implemented converter/CI exchange schema

`tinyvm cartridge check-profile GAME.wasm APP.tahost --json` emits one compact
UTF-8 JSON object followed by a newline. It compares a standards-valid cartridge
with one exact canonical TAH1 app-build profile without instantiating the module,
running its start function or calling native code.

The command keeps the existing text output when `--json` is absent. With
`--json`, stdout contains only the report and stderr remains empty for all
reportable outcomes:

```text
exit 0  valid cartridge/profile and compatible=true
exit 1  valid but incompatible, or invalid input
```

An invocation/usage error is not a report and continues to print usage on
stderr.

## Schema tree

```text
tinyarcade-host-compatibility-report
├── schema                                      string, fixed name
├── schema_version                             integer, 1
├── valid                                      boolean
├── compatible                                 boolean
├── error                                      string, invalid reports only
├── cartridge                                  valid reports only
│   ├── game_id / game_version                 string
│   ├── abi_version / state_version            integer
│   ├── wasm_bytes                             integer
│   └── native_capabilities                    string[]
├── host_profile
│   └── bytes                                  integer
├── wasm_features                              string[]
├── unsupported_features                       string[]
├── function_imports                           object[]
│   ├── module / field / class                 string
│   ├── params / results                       integer
│   └── i32_only                               boolean
├── issues                                     object[]
│   ├── kind                                   missing_function | signature_mismatch
│   ├── module / field                         string
│   ├── required_params / required_results     integer
│   └── available_params / available_results   integer | null
└── issue_count                                integer
```

`issue_count` is the sum of `unsupported_features.length` and `issues.length`.
Unsupported standard-Wasm families stay separate from native-function issues so
a creator can suggest a compiler-profile change without pretending it is a
missing host import. `available_*` is `null` only for a wholly missing function;
a same-name wrong signature reports both available arities.

For malformed, oversized, non-regular or otherwise undecodable input, the
report contains only the fixed schema identity, `valid=false`,
`compatible=false` and one non-empty escaped `error`. JSON strings escape every
control byte, quote and backslash; arrays retain canonical descriptor/profile
order. The schema does not include file paths, timestamps or host callbacks, so
identical input bytes produce identical JSON.

This is static compatibility evidence. Step limits, frame/audio/state validity,
suspend/resume determinism and native callback behavior remain owned by
`tinyvm cartridge check`, replay gates and app/runtime tests.
