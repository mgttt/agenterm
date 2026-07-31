//! Script Runtime HTTP host integration behind the Platform Facade.

use ureq::{Error as UreqError, tls::TlsConfig};

pub(crate) fn tls_config() -> TlsConfig {
    crate::platform::selected::script_http::tls_config()
}

pub(crate) fn is_platform_tls_error(error: &UreqError) -> bool {
    crate::platform::selected::script_http::is_platform_tls_error(error)
}
