# Chassis-L2

L2 is the Host ABI (`host-abi.json`) plus optional bytecode programs.
Capability names here are what Chassis-L3 may call.

The catalog classifies `fleet.*`, tab, clipboard, Control Center, and
computer-use as L2. Computer-use is the exceptional **rare native L2 plugin**
(`cu`), never part of L1. An optional plugin being absent is compatibility and
availability information; capability metadata is discovery, not permission.

Daily updates are **file replace + compose**, not `rustc`. Native plugins
(`cu`) are the only rustc exception. No libtcc.

`impl: "host"` means the current L1/workbench still implements the name.
`impl: "chassis"` is answered by this crate. OS library names and `dlcall`
do not belong in this table.

`programs/active-tab.json` is the first non-toy replaceable L2 artifact. It
routes the stable `tabs.active` Host ABI operation and returns the compact VM
result supplied by the host. The embedded expected bytecode is evidence that
the same file assembles deterministically; compose copies it unchanged.
