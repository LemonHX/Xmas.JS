use std::{convert::Infallible, sync::LazyLock};

use super::dns_cache::CachedDnsResolver;
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper_rustls::HttpsConnector;
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::{TokioExecutor, TokioTimer},
};
use rustls::ClientConfig;

use crate::tls::config::{build_client_config, BuildClientConfigOptions};

pub type HyperClient =
    Client<HttpsConnector<HttpConnector<CachedDnsResolver>>, BoxBody<Bytes, Infallible>>;

pub fn build_client(
    tls_config: Option<ClientConfig>,
) -> Result<HyperClient, Box<dyn std::error::Error + Send + Sync>> {
    let config = if let Some(tls_config) = tls_config {
        tls_config
    } else {
        build_client_config(BuildClientConfigOptions {
            reject_unauthorized: false,
            ca: None,
        })?
    };

    let builder = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_or_http();

    let mut cache_dns_connector = CachedDnsResolver::new().into_http_connector();
    cache_dns_connector.enforce_http(false);

    let https = builder
        .enable_all_versions()
        .wrap_connector(cache_dns_connector);

    Ok(Client::builder(TokioExecutor::new())
        .pool_timer(TokioTimer::new())
        .build(https))
}
