use ureq::{
    Error as UreqError,
    tls::{RootCerts, TlsConfig, TlsProvider},
};

pub(crate) fn tls_config() -> TlsConfig {
    TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(RootCerts::WebPki)
        .build()
}

pub(crate) fn is_platform_tls_error(error: &UreqError) -> bool {
    matches!(error, UreqError::Rustls(_))
}
