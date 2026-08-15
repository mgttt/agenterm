<!--
CU hand: none — a Mach host port is a kernel send right, not a window fact.
No script probe: dyn's generic dlcall surface has no typed Mach-port owner or
release operation.
-->

# `mach_host_self` is intentionally not a `dlcall` example

`mach_host_self()` returns a Mach send right. Calling it allocates a right that
must be released with the matching Mach ownership operation. `agenterm-dyn`
catalogues the symbol as a Darwin placeholder, but does not expose a successful
`dlcall` form: the generic pointer/result door cannot own or release that right.

Use a typed Mach adapter with an explicit release owner before adding a live
probe. Until then, absence of a Lisp form is the correct resource-safety
contract.
