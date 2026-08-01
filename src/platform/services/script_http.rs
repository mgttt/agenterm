//! Script Runtime HTTP host integration behind the Platform Facade.

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

pub(crate) fn tls_config() -> Result<TlsConfig, &'static str> {
    let (provider, roots) = match agenterm_platform::platform_kind() {
        agenterm_platform::PlatformKind::Windows => {
            (TlsProvider::NativeTls, RootCerts::PlatformVerifier)
        }
        agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos => {
            (TlsProvider::Rustls, RootCerts::WebPki)
        }
        _ => return Err("http_tls_backend_unsupported"),
    };
    Ok(TlsConfig::builder()
        .provider(provider)
        .root_certs(roots)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_provider_matches_the_selected_platform() {
        let config = tls_config().expect("supported platform TLS config");
        match agenterm_platform::platform_kind() {
            agenterm_platform::PlatformKind::Windows => {
                assert_eq!(config.provider(), TlsProvider::NativeTls);
                assert!(matches!(config.root_certs(), &RootCerts::PlatformVerifier));
            }
            agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos => {
                assert_eq!(config.provider(), TlsProvider::Rustls);
                assert!(matches!(config.root_certs(), &RootCerts::WebPki));
            }
            _ => panic!("unsupported test platform"),
        }
    }
}
