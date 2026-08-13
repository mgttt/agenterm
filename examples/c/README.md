# libagenterm C consumer probe

`examples/c/agenterm_probe.c` is a self-contained C consumer of the
`libagenterm` C ABI (`crates/agenterm-abi`). It `#include "agenterm.h"` from
the repository-root `include/` directory and exercises version / build id,
capability negotiation, process enumeration (two-stage probe) and the
thread-local error record. Any step that violates the ABI contract exits 1
with a reason on stderr.

It is also compiled, linked and run by the link-time regression
`crates/agenterm-abi/tests/c_consumer.rs` on every `cargo test`.

## Prerequisites

A C compiler and a built cdylib:

```
cargo build -p agenterm-abi --profile abi-dev
```

The cdylib (and on Windows the import library `agenterm.dll.lib`) lands
in `target/abi-dev/`.

## MSVC (cl.exe)

```
cl /nologo /W4 /WX /Iinclude examples/c/agenterm_probe.c target/abi-dev/agenterm.dll.lib /Fe:probe.exe
```

## gcc / clang (Unix)

```
cc -Wall -Wextra -Werror -Iinclude examples/c/agenterm_probe.c -o probe -Ltarget/abi-dev -lagenterm
```

## Static link (no DLL, self-contained executable)

Link the staticlib instead of the import library / shared object. A Rust
`staticlib` needs the C side to supply the Rust runtime's system libraries;
the MSVC list below is measured (see
`crates/agenterm-abi/README.md` — 静态链接), the Unix list is the initial set
for CI to calibrate. Run needs no DLL and no `LD_LIBRARY_PATH`:

```
cl /nologo /W4 /WX /Iinclude examples/c/agenterm_probe.c target/abi-dev/agenterm.lib ws2_32.lib ntdll.lib ole32.lib user32.lib uxtheme.lib dwmapi.lib /Fe:probe.exe   (Windows/MSVC, after vcvars64)
cc -Wall -Wextra -Werror -Iinclude examples/c/agenterm_probe.c target/abi-dev/libagenterm.a -o probe -ldl -lpthread -lm   (Linux)
```

This is the same `agenterm_probe.c` exercised by
`crates/agenterm-abi/tests/c_static_link.rs`, which fails the build if the
static link cannot be made to work.

## Run

`agenterm.dll` must sit next to the executable (Windows searches the
executable's own directory first):

```
copy target\abi-dev\agenterm.dll probe.exe    (Windows)
probe.exe
```

On Unix the dynamic loader does not search the executable's directory, so
point it at the cdylib instead:

```
LD_LIBRARY_PATH=target/abi-dev ./probe            (Linux)
DYLD_LIBRARY_PATH=target/abi-dev ./probe          (macOS)
```

## Expected output

```
abi_version=0x00010000
build_id=0.1.16+abi.1
capability(PTY)=0
capability(WINDOW_HOST)=0
capability(SCREENSHOT)=0
capability(PROCESS_OBSERVE)=0
self_pid=<pid>
process_list probe: need=<n>
proc[0] id=<pid> parent_id=<pid> name=<name>
proc[1] id=<pid> parent_id=<pid> name=<name>
proc[2] id=<pid> parent_id=<pid> name=<name>
last_error: operation=agt_process_list code=buffer_too_small message=<...>
```

Each `capability(...)` line prints the raw status: `0` (`AGT_OK`) or `1`
(`AGT_UNSUPPORTED`). Both are legitimate per the
`agt_capability_query` contract; the probe fails only on `AGT_FAILED` or any
other unexpected value. Platform differences show up here — for example
`capability(WINDOW_HOST)=1` on macOS, where the AppKit-based window mechanism
requires the main thread.

The final `last_error` line shows the two-stage probe's `buffer_too_small`
record — that is the documented negotiation signal, not a failure. The probe
program exits 0.
