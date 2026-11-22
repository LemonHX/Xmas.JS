//! The one place the package manager talks to a registry.
//!
//! This is the same stack the runtime serves fetch from: trillium-client over trillium-smol, with
//! rustls and the rustls-rustcrypto provider and the webpki roots compiled in, so nothing here
//! needs a C toolchain or reads the host's certificate store.
//!
//! A call is an ordinary task on smol's executor and costs no thread while it waits, so how much
//! of an install runs at once is decided by the permit count below rather than by the core count.

use async_lock::Semaphore;
use smol::io::AsyncReadExt;
use color_eyre::eyre::{eyre, Result};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use std::sync::{Arc, LazyLock};
use trillium_client::{Client, Method};
use trillium_rustls::RustlsConfig;
use trillium_smol::ClientConfig as TcpClient;

use crate::package::config::RegistryAuth;

/// Requests on the wire at once. A dependency tree is wide and every request is a socket wait,
/// so this is the number that decides how long a cold install takes.
pub const IN_FLIGHT: usize = 100;

static PERMITS: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(IN_FLIGHT));

/// What actually happened, printed when `XMAS_PM_TRACE` is set: how many requests were made and
/// how many were ever on the wire at the same time.
static REQUESTS: AtomicUsize = AtomicUsize::new(0);
static IN_FLIGHT_NOW: AtomicUsize = AtomicUsize::new(0);
static HIGH_WATER: AtomicUsize = AtomicUsize::new(0);

static BYTES: AtomicUsize = AtomicUsize::new(0);

struct Traced;

impl Drop for Traced {
    fn drop(&mut self) {
        IN_FLIGHT_NOW.fetch_sub(1, Ordering::SeqCst);
    }
}

fn trace_request() -> Option<Traced> {
    if std::env::var_os("XMAS_PM_TRACE").is_none() {
        return None;
    }

    static TICKER: std::sync::Once = std::sync::Once::new();
    TICKER.call_once(|| {
        smol::spawn(async {
            let start = std::time::Instant::now();
            loop {
                smol::Timer::after(std::time::Duration::from_secs(2)).await;
                eprintln!(
                    "[{:>5.1}s] requests={} in-flight={} {:.1} MB",
                    start.elapsed().as_secs_f64(),
                    REQUESTS.load(Ordering::SeqCst),
                    IN_FLIGHT_NOW.load(Ordering::SeqCst),
                    BYTES.load(Ordering::SeqCst) as f64 / (1024.0 * 1024.0),
                );
            }
        })
        .detach();
    });
    REQUESTS.fetch_add(1, Ordering::SeqCst);
    let live = IN_FLIGHT_NOW.fetch_add(1, Ordering::SeqCst) + 1;
    HIGH_WATER.fetch_max(live, Ordering::SeqCst);
    Some(Traced)
}

pub fn trace_report() {
    if std::env::var_os("XMAS_PM_TRACE").is_some() {
        eprintln!(
            "requests: {}, in flight at once at most: {}",
            REQUESTS.load(Ordering::SeqCst),
            HIGH_WATER.load(Ordering::SeqCst),
        );
        eprintln!(
            "downloaded: {:.1} MB",
            BYTES.load(Ordering::SeqCst) as f64 / (1024.0 * 1024.0)
        );
    }
}

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
            // The abbreviated document is the one an installer is supposed to ask for: it drops
            // the readme, the manual and every manifest field an install never reads. On this
            // tree the full documents are about twice the size, and typescript's alone is
            // sixteen megabytes.
            .with_default_header("accept", "application/vnd.npm.install-v1+json")
            // Sixteen of the twenty megabytes a cold install moves are registry documents, and
            // they are JSON: asked for compressed they arrive about a tenth of the size. No
            // client sends this on its own, and without it an install spends its time on text
            // that compresses to nothing.
            .with_default_header("accept-encoding", "gzip")
    });
    &CLIENT
}

/// An install waits for its slowest request, so one stalled socket is the whole install stalled:
/// every wait inside a request is given a limit of its own and turns into an error the retry loop
/// can act on, instead of a permit held until the other end of the world times out.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const IDLE_TIMEOUT: Duration = Duration::from_secs(15);

async fn bounded<T>(
    url: &str,
    label: &str,
    limit: Duration,
    work: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    smol::future::or(work, async {
        smol::Timer::after(limit).await;
        Err(eyre!("{url}: {label} within {}s", limit.as_secs()))
    })
    .await
}

/// Fetch one URL and return its body.
///
/// One hop only: a registry that answers a redirect with a body we would then treat as a
/// tarball is a registry we want to hear about, not one to follow quietly.
pub async fn get(url: &str, auth: Option<&RegistryAuth>) -> Result<Vec<u8>> {
    let parsed = url::Url::parse(url).map_err(|error| eyre!("invalid URL {url}: {error}"))?;

    // Held until the body is in: the connection is what the permit is protecting, and a request
    // that dropped it after the headers would still be a socket in flight.
    let _permit = PERMITS.acquire().await;
    let _trace = trace_request();

    let mut conn = client().build_conn(Method::Get, parsed);
    if let Some(auth) = auth {
        let (name, value) = auth.header()?;
        conn = conn.with_request_header(name, value);
    }

    let mut conn = bounded(url, "no answer", CONNECT_TIMEOUT, async {
        conn.await.map_err(|error| eyre!("{url}: {error}"))
    })
    .await?;

    let status = conn.status().map(u16::from).unwrap_or(0);
    if !(200..300).contains(&status) {
        return Err(eyre!("{url} answered {status}"));
    }

    // A registry answers with `application/json` for a package with a long release history and
    // that document is tens of megabytes. Trillium caps a received body at ten by default, which
    // is a sane cap for an application and the wrong one for a registry client.
    let mut body = conn
        .take_response_body()
        .ok_or_else(|| eyre!("{url} answered without a body"))?
        .with_max_len(512 * 1024 * 1024);

    let mut bytes = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let read = bounded(url, "no data", IDLE_TIMEOUT, async {
            body.read(&mut chunk).await.map_err(|error| eyre!("{url}: {error}"))
        })
        .await?;

        if read == 0 {
            break;
        }

        bytes.extend_from_slice(&chunk[..read]);
    }

    BYTES.fetch_add(bytes.len(), Ordering::Relaxed);

    let compressed = conn
        .response_headers()
        .get_str("content-encoding")
        .map(str::to_owned);

    match compressed.as_deref() {
        Some("gzip") | Some("x-gzip") => Ok(gunzip(&bytes)?),
        Some("deflate") => Ok(inflate(&bytes)?),
        _ => Ok(bytes),
    }
}

/// A gzip body, back to what the registry meant to send.
fn gunzip(bytes: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(bytes)
        .read_to_end(&mut out)
        .map_err(|error| eyre!("a gzip body did not decode: {error}"))?;
    Ok(out)
}

/// A deflate body, back to what the registry meant to send.
fn inflate(bytes: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(bytes)
        .read_to_end(&mut out)
        .map_err(|error| eyre!("a deflate body did not decode: {error}"))?;
    Ok(out)
}
