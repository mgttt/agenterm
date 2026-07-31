use ureq::{
    Error as UreqError,
    tls::{RootCerts, TlsConfig, TlsProvider},
};

pub(crate) fn tls_config() -> TlsConfig {
    TlsConfig::builder()
        .provider(TlsProvider::NativeTls)
        .root_certs(RootCerts::PlatformVerifier)
        .build()
}

pub(crate) fn is_platform_tls_error(error: &UreqError) -> bool {
    matches!(error, UreqError::NativeTls(_) | UreqError::Der(_))
}
