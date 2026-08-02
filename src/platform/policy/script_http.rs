//! Product Script Runtime HTTP TLS policy.
//!
//! The native TLS mechanisms live in agenterm-platform/ureq; this table is
//! the AgenTerm-level decision for provider and trusted-root shape.

use super::host::{is_unix_host, is_windows_host};

pub(crate) fn script_http_tls_config() -> Result<ureq::tls::TlsConfig, &'static str> {
    let (provider, roots) = if is_windows_host() {
        (
            ureq::tls::TlsProvider::NativeTls,
            ureq::tls::RootCerts::PlatformVerifier,
        )
    } else if is_unix_host() {
        (ureq::tls::TlsProvider::Rustls, ureq::tls::RootCerts::WebPki)
    } else {
        return Err("http_tls_backend_unsupported");
    };
    Ok(ureq::tls::TlsConfig::builder()
        .provider(provider)
        .root_certs(roots)
        .build())
}

#[allow(dead_code)]
pub(crate) fn script_http_tls_provider() -> ureq::tls::TlsProvider {
    if is_windows_host() {
        ureq::tls::TlsProvider::NativeTls
    } else {
        ureq::tls::TlsProvider::Rustls
    }
}

#[allow(dead_code)]
pub(crate) fn script_http_tls_root_certs_are_expected(root_certs: &ureq::tls::RootCerts) -> bool {
    if is_windows_host() {
        matches!(root_certs, ureq::tls::RootCerts::PlatformVerifier)
    } else {
        matches!(root_certs, ureq::tls::RootCerts::WebPki)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        script_http_tls_config, script_http_tls_provider, script_http_tls_root_certs_are_expected,
    };
    use crate::platform::policy::host::{is_unix_host, is_windows_host};

    #[test]
    fn tls_provider_and_roots_follow_runtime_kind() {
        let provider = script_http_tls_provider();
        let expected_roots = if is_windows_host() {
            ureq::tls::RootCerts::PlatformVerifier
        } else {
            ureq::tls::RootCerts::WebPki
        };
        assert!(script_http_tls_root_certs_are_expected(&expected_roots));
        assert_eq!(
            provider,
            if is_windows_host() {
                ureq::tls::TlsProvider::NativeTls
            } else {
                ureq::tls::TlsProvider::Rustls
            }
        );
        assert!(script_http_tls_config().is_ok() || !is_windows_host() && !is_unix_host());
    }
}
