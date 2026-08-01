use crate::control_window::{ControlWindowApplication, ControlWindowError, ControlWindowOptions};

pub(crate) fn run_control_window(
    _options: ControlWindowOptions,
    _application: Box<dyn ControlWindowApplication>,
) -> Result<(), ControlWindowError> {
    Err(ControlWindowError::Unsupported {
        reason: "native-control-window-not-implemented-for-linux".into(),
    })
}
