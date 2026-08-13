//! `agenterm-cu` — computer-use foundation (PRD_02_28/29/30/31).
//!
//! Orchestrator agents should drive desktops through structured observation and
//! actuation, not screenshot/OCR coordinate guessing. See `README.md`.

pub mod audit;
pub mod auth;
#[cfg(target_os = "macos")]
pub mod ax_guide;
pub mod command;
pub mod dynlib;
pub mod executor;
pub mod hotkeys;
pub mod mechanism;
pub mod place;
pub mod reply;
#[cfg(target_os = "macos")]
pub mod status_menu;
pub mod target;

pub use auth::{Authorization, Grant};
pub use command::{Command, PointerButton, WaitCondition};
pub use executor::Executor;
pub use reply::{CuError, CuReply};
pub use target::TargetRef;
