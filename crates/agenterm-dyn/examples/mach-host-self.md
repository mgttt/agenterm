<!--
CU hand: windows — Mach host port is a host identity token, not a window.
Script probe only: dyn does not open Mach IPC or cu sessions.
-->

# Read the host port with `mach_host_self`

macOS example. `mach_host_self(void) -> mach_port_t` is represented by the
bounded unsigned 32-bit return lane.

```lisp
(dlcall "libSystem.B.dylib" "mach_host_self" "u32")
```

The result is a non-zero Mach host port. Adjacent calls return the same
value; this is a native fact probe, not a Mach IPC session.
