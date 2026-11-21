//! The one place the package manager talks to a registry.
//!
//! This is the same stack the runtime serves fetch from: trillium-client over trillium-smol,
//! with rustls and the rustls-rustcrypto provider and the webpki roots compiled in, so nothing
//! here needs a C toolchain or reads the host's certificate store. There is no async runtime in
//! this crate; a call blocks the thread it is on and the work pool decides how many of those
//! run at once.

use color_eyre::eyre::{eyre, Result};
use std::sync::{Arc, LazyLock};
use trillium_client::{Client, Method};
use trillium_rustls::RustlsConfig;
use trillium_smol::ClientConfig as TcpClient;

use crate::config::RegistryAuth;

fn tls() -> Arc<rustls::ClientConfig> {
    let provider = Arc::new(rustls_rustcrypto::provider());
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    Arc::new(
        rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("the provider supports the default protocol versions")
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

fn client() -> &'static Client {
    static CLIENT: LazyLock<Client> = LazyLock::new(|| {
        Client::new(RustlsConfig::new(tls(), TcpClient::default()))
            .with_default_header("accept", "application/json")
    });
    &CLIENT
}

/// Fetch one URL and return its body.
///
/// One hop only: a registry that answers a redirect with a body we would then treat as a
/// tarball is a registry we want to hear about, not one to follow quietly.
pub fn get(url: &str, auth: Option<&RegistryAuth>) -> Result<Vec<u8>> {
    let parsed = url::Url::parse(url).map_err(|error| eyre!("invalid URL {url}: {error}"))?;
    let mut conn = client().build_conn(Method::Get, parsed);
    if let Some(auth) = auth {
        let (name, value) = auth.header()?;
        conn = conn.with_request_header(name, value);
    }

    // Conn is IntoFuture rather than a future, so awaiting it is the block_on body.
    let mut conn = smol::block_on(async { conn.await }).map_err(|error| eyre!("{url}: {error}"))?;
    let status = conn.status().map(u16::from).unwrap_or(0);
    if !(200..300).contains(&status) {
        return Err(eyre!("{url} answered {status}"));
    }

    let body = conn
        .take_response_body()
        .ok_or_else(|| eyre!("{url} answered without a body"))?;
    smol::block_on(body.read_bytes()).map_err(|error| eyre!("{url}: {error}"))
}
