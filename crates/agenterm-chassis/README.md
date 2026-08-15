# agenterm-chassis

Independent **Chassis-L1 / L2 / L3** product image. Not a command-line shell.
Does not depend on the workbench `agenterm` package.

| Layer | In this crate | Role |
|-------|----------------|------|
| L1 | `compose` copies `l1/<cell>/loader` bytes | six frozen loaders |
| L2 | `l2/host-abi.json` + bytecode VM | capability names L3 may call |
| L3 | `l3/example-app.json` | app names only those capabilities |

```text
L2 programs: JSON IR → assemble() → run()
```

L2 is a **tiny custom-ISA AOT → bytecode → bounded VM**. rustc stays on
Chassis-L1 (rare, six-cell) and rare native L2 plugins (cu). Rejected for L2:

- libtcc / embedded C compiler
- rustc of L2 on the daily path
- cranelift / LLVM JIT
- dyn `dlcall` from L3

```text
cargo run -p agenterm-chassis -- native-cell
cargo run -p agenterm-chassis -- compose --from <layout> --out <image>
cargo run -p agenterm-chassis -- check <image>
cargo run -p agenterm-chassis -- inspect <image>
```

Daily loop is compose/check, not rustc of the workbench. This crate is the
extracted frame; the live `agenterm` PE is not yet replaced by these loaders.
