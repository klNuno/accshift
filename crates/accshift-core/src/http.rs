//! Where every HTTP client starts. reqwest 0.13 is built with
//! `rustls-no-provider`, so ring is installed as the process-wide rustls
//! provider before the first client, the provider reqwest 0.12's
//! `rustls-tls` built in, rather than reqwest 0.13's default aws-lc-rs.

use std::sync::Once;

fn install_crypto_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // Err only when another provider is already the default: kept.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// `reqwest::Client::builder()` once the crypto provider is installed.
pub fn client_builder() -> reqwest::ClientBuilder {
    install_crypto_provider();
    reqwest::Client::builder()
}

/// `reqwest::blocking::Client::builder()` once the crypto provider is
/// installed.
pub fn blocking_client_builder() -> reqwest::blocking::ClientBuilder {
    install_crypto_provider();
    reqwest::blocking::Client::builder()
}

#[cfg(test)]
mod tests {
    #[test]
    fn both_builders_build_a_client() {
        super::client_builder().build().unwrap();
        super::blocking_client_builder().build().unwrap();
    }
}
