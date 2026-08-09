//! Compatibility projection for host filesystem conventions.
//!
//! Product modules consume this facade only; operating-system conventions live
//! behind `services::paths` and the selected adapter.

pub(crate) use crate::platform::services::paths::{
    control_center_executable_name, default_workspace_path, instance_registry_dir, settings_path,
    terminal_default_font_size,
};
