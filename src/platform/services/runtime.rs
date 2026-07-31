//! OS-neutral runtime-default service.

use crate::platform::selected;

pub(crate) fn default_terminal_shell() -> String {
    selected::runtime::default_terminal_shell()
}
