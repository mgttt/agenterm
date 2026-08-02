//! Host-specific frontend adapters (physical tree = module ownership).
//!
//! Both host trees stay declared so product ingress can dispatch without
//! `cfg(target_*)` in the main crate (native OS markers stay in
//! `agenterm-platform`). Host selection is runtime via `FrontendHost`.

#[allow(dead_code)]
pub(crate) mod unix;
#[allow(dead_code)]
pub(crate) mod windows;
