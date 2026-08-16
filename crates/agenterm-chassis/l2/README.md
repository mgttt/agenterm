# Chassis-L2

L2 is the Host ABI (`host-abi.json`) plus optional bytecode programs.
Capability names here are what Chassis-L3 may call. The catalog binds every
name to Host ABI version 3 and to a named machine-readable signature; unknown
names, signatures, versions, extra fields, and out-of-range values fail closed.

The catalog classifies `fleet.*`, tab, clipboard, Control Center, and
computer-use as L2. Computer-use is the exceptional **rare native L2 plugin**
(`cu`), never part of L1. An optional plugin being absent is compatibility and
availability information; capability metadata is discovery, not permission.
The `authorization.encoded: false` invariant keeps authorization in workbench
policy outside the chassis. In particular, discovering an optional `cu` plugin
does not authorize it and absence never falls back to another mechanism.

Daily updates are **file replace + compose**, not `rustc`. Native plugins
(`cu`) are the only rustc exception. No libtcc.

`impl: "host"` means the current L1/workbench still implements the name.
`impl: "chassis"` is answered by this crate. OS library names and `dlcall`
do not belong in this table.

The bounds mirror the current workbench contract: event waits are at most 60
seconds, terminal capture at most 1 MiB, clipboard paste at most 256 KiB,
tab notes at most 4 KiB, tabs width is 180..480 pixels, and a Host ABI JSON
reply at most 8 MiB. Pointer and
wheel signatures use the workbench's pixel-coordinate request fields rather
than the older script-wrapper shorthand.

`programs/active-tab.json` is the first non-toy replaceable L2 artifact. It
routes the stable `tabs.active` Host ABI operation and returns the compact VM
result supplied by the host. The embedded expected bytecode is evidence that
the same file assembles deterministically; compose copies it unchanged.
