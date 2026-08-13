//! Spectacle-compatible named window placement (PRD_02_32).
//!
//! Pure geometry lives here with no OS imports. Apply goes through
//! `agenterm-platform`.

mod action;
mod apply;
mod geometry;
mod history;

pub use apply::{apply_rect, read_rect, rect_from_bounds, screen_from_info};

pub use action::PlaceAction;
pub use geometry::{Rect, Screen, place};
pub use history::PlaceHistory;
