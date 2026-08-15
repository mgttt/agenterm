//! `agenterm-cu` — computer-use foundation (PRD_02_28/29/30/31).
//!
//! Orchestrator agents should drive desktops through structured observation and
//! actuation, not screenshot/OCR coordinate guessing. See `README.md`.

pub mod audit;
pub mod auth;
pub mod auth_store;
#[cfg(target_os = "macos")]
pub mod ax_guide;
pub mod command;
pub mod dynlib;
pub mod executor;
pub mod grant_management;
pub mod host_actions;
pub mod hotkeys;
pub mod mechanism;
pub mod place;
pub mod rdp_transport;
pub mod reply;
pub mod ssh_transport;
#[cfg(target_os = "macos")]
pub mod status_menu;
pub mod target;
pub mod target_binding;
pub mod vnc_transport;

pub use auth::{Authorization, Grant};
pub use command::{Command, PointerButton, WaitCondition};
pub use executor::Executor;
pub use rdp_transport::RdpEndpoint;
pub use reply::{CuError, CuReply};
pub use ssh_transport::SshEndpoint;
pub use target::TargetRef;
pub use vnc_transport::VncEndpoint;
