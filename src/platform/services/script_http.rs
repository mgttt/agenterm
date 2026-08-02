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
        assert_eq!(
            config.provider(),
            crate::platform::script_http_tls_provider()
        );
        assert!(crate::platform::script_http_tls_root_certs_are_expected(
            config.root_certs()
        ));
    }
}
