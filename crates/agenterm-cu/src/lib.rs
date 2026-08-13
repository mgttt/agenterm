//! `agenterm-cu` — computer-use foundation (PRD_02_28/29/30/31).
//!
//! Orchestrator agents should drive desktops through structured observation and
//! actuation, not screenshot/OCR coordinate guessing. See `README.md`.

pub mod auth;
pub mod audit;
pub mod command;
pub mod executor;
pub mod reply;
pub mod target;

pub use auth::{Authorization, Grant};
pub use command::{Command, PointerButton, WaitCondition};
pub use executor::Executor;
pub use reply::{CuError, CuReply};
pub use target::TargetRef;
