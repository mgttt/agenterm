# agenterm-platform

`agenterm-platform` is AgenTerm's reusable Rust boundary for typed operating-
system capabilities. It contains platform-neutral contracts, capability
facades, one private target selector, and Windows/Linux/macOS adapters. Product
policy, UI state, Fleet behavior, and AgenTerm executable naming stay in the
embedding application.

The crate is under active development. Pin an exact Git revision when consuming
it from another repository.
