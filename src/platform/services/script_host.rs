//! Whether the native CLI host exposes the Script worker embedding path.

use crate::platform::selected;

pub(crate) const fn hosted_worker_available() -> bool {
    selected::script_host::HOSTED_WORKER_AVAILABLE
}
