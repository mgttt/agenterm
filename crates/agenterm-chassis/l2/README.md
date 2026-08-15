# Chassis-L2

L2 is the Host ABI (`host-abi.json`) plus optional bytecode programs.
Capability names here are what Chassis-L3 may call.

Daily updates are **file replace + compose**, not `rustc`. Native plugins
(`cu`) are the only rustc exception. No libtcc.

`impl: "host"` means the current L1/workbench still implements the name.
`impl: "chassis"` is answered by this crate. OS library names and `dlcall`
do not belong in this table.
