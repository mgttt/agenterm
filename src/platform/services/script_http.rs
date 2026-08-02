//! Script Runtime HTTP host integration behind the Platform Facade.

pub(crate) fn tls_config() -> Result<ureq::tls::TlsConfig, &'static str> {
    crate::platform::script_http_tls_config()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_provider_matches_the_selected_platform() {
        let config = tls_config().expect("supported platform TLS config");
        if crate::platform::is_windows_host() {
                assert_eq!(config.provider(), ureq::tls::TlsProvider::NativeTls);
                assert!(matches!(
                    config.root_certs(),
                    &ureq::tls::RootCerts::PlatformVerifier
                ));
        } else {
                assert_eq!(config.provider(), ureq::tls::TlsProvider::Rustls);
                assert!(matches!(config.root_certs(), &ureq::tls::RootCerts::WebPki));
        }
    }
}
