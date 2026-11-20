//! The HTTP client fetch stands on: one trillium client per realm thread.
//!
//! trillium-client picks the protocol: HTTP/1.1 over http, and over https whatever ALPN
//! agrees on (h2 when the server offers it, h1.1 otherwise). Connections are pooled per
//! origin, and the pool lives on the thread that made the request, so a realm thread never
//! shares a socket with another one.
//!
//! TLS is rustls with the rustls-rustcrypto provider and the webpki roots compiled in. No
//! system certificate store is read and no C compiler is needed to build it.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rustls::ClientConfig as TlsClient;
use rustls::RootCertStore;
pub use trillium_client::ResponseBody;
use trillium_client::{Client, Method};
use trillium_rustls::RustlsConfig;
use trillium_smol::ClientConfig as TcpClient;

/// How many redirects fetch follows before giving up, the same limit the web platform sets.
const MAX_REDIRECTS: usize = 20;

/// The message an aborted request settles with, word for word what the platform reports.
pub const ABORTED: &str = "The operation was aborted";

thread_local! {
    /// Built on the first request of a thread. A thread that never fetches never builds it.
    static CLIENT: RefCell<Option<Client>> = const { RefCell::new(None) };
}

/// What one request asks for.
pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// What came back. The body is a stream: the caller reads it at its own speed.
pub struct Response {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub url: String,
    pub redirected: bool,
    pub encoding: Encoding,
    pub body: ResponseBody<'static>,
}

/// The content codings we know. Anything else is handed over untouched, as it arrived.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encoding {
    Identity,
    Gzip,
    Deflate,
    Brotli,
    Unknown,
}

impl Encoding {
    fn of(headers: &[(String, String)]) -> Encoding {
        let value = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-encoding"))
            .map(|(_, value)| value.trim().to_ascii_lowercase())
            .unwrap_or_default();
        match value.as_str() {
            "" | "identity" => Encoding::Identity,
            "gzip" | "x-gzip" => Encoding::Gzip,
            "deflate" => Encoding::Deflate,
            "br" => Encoding::Brotli,
            _ => Encoding::Unknown,
        }
    }
}

fn tls() -> Arc<TlsClient> {
    let provider = Arc::new(rustls_rustcrypto::provider());
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    Arc::new(
        TlsClient::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("the provider supports the default protocol versions")
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

fn client() -> Client {
    CLIENT.with(|slot| {
        slot.borrow_mut()
            .get_or_insert_with(|| Client::new(RustlsConfig::new(tls(), TcpClient::default())))
            .clone()
    })
}

/// Send one request, following redirects when asked.
///
/// The cancel flag is checked between hops and between body reads. A request that is aborted
/// while it waits for its response headers is not interrupted; its answer is thrown away when
/// it arrives, which is the observable part.
pub async fn send(
    request: Request,
    follow: bool,
    cancel: Arc<AtomicBool>,
) -> Result<Response, String> {
    let original = request.url.clone();
    let mut method = request.method.clone();
    let mut url = request.url.clone();
    let mut headers = request.headers.clone();
    let mut body = request.body.clone();
    let mut redirected = false;
    let mut hops = 0;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(ABORTED.to_string());
        }

        let parsed =
            url::Url::parse(&url).map_err(|error| format!("Failed to parse URL: {error}"))?;
        let method_value =
            Method::try_from(method.as_str()).map_err(|_| format!("Invalid method {method}"))?;
        let mut conn = client().build_conn(method_value, parsed.clone());
        for (name, value) in &headers {
            conn = conn.with_request_header(name.clone(), value.clone());
        }
        if let Some(bytes) = body.clone() {
            conn = conn.with_body(trillium_client::Body::new_static(bytes));
        }

        let mut conn = conn.await.map_err(|error| error.to_string())?;
        let status = conn
            .status()
            .ok_or_else(|| "the response has no status".to_string())?;
        let code = u16::from(status);
        let status_text = status.canonical_reason().to_string();
        let response_headers = collect(conn.response_headers());

        if follow && matches!(code, 301 | 302 | 303 | 307 | 308) {
            if let Some(location) = header(&response_headers, "location") {
                hops += 1;
                if hops > MAX_REDIRECTS {
                    return Err("redirect count exceeded".to_string());
                }
                let next = parsed
                    .join(&location)
                    .map_err(|error| format!("invalid redirect location: {error}"))?;
                // 303 always turns into a GET; 301 and 302 do the same for a POST, which is
                // what every browser does and what the fetch specification says.
                if code == 303 || (matches!(code, 301 | 302) && method.eq_ignore_ascii_case("post"))
                {
                    method = "GET".to_string();
                    body = None;
                    headers.retain(|(name, _)| {
                        !name.eq_ignore_ascii_case("content-type")
                            && !name.eq_ignore_ascii_case("content-length")
                    });
                }
                if next.origin() != parsed.origin() {
                    headers.retain(|(name, _)| !name.eq_ignore_ascii_case("authorization"));
                }
                if cancel.load(Ordering::Relaxed) {
                    return Err(ABORTED.to_string());
                }
                redirected = true;
                url = next.to_string();
                continue;
            }
        }

        let encoding = Encoding::of(&response_headers);
        let body = conn
            .take_response_body()
            .ok_or_else(|| "the response has no body".to_string())?;

        return Ok(Response {
            status: code,
            status_text,
            headers: response_headers,
            url: if redirected { url } else { original },
            redirected,
            encoding,
            body,
        });
    }
}

fn collect(headers: &trillium_client::Headers) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, values) in headers.iter() {
        let name = name.to_string();
        for value in values.iter() {
            out.push((name.clone(), value.as_str().unwrap_or_default().to_string()));
        }
    }
    out
}

fn header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_lite::AsyncReadExt;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// The same request, but started from smol::spawn and read from another thread, which is
    /// the shape the engine uses.
    #[test]
    fn one_request_from_a_spawned_task() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut buffer = [0u8; 4096];
            let _ = socket.read(&mut buffer);
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                )
                .expect("write");
            socket.flush().ok();
        });

        let (tx, rx) = smol::channel::bounded::<Vec<u8>>(4);
        smol::spawn(async move {
            let request = Request {
                method: "GET".to_string(),
                url: format!("http://127.0.0.1:{port}/"),
                headers: Vec::new(),
                body: None,
            };
            let cancel = Arc::new(AtomicBool::new(false));
            let response = send(request, true, cancel).await.expect("response");
            let mut body = response.body;
            let mut buffer = vec![0u8; 16 * 1024];
            loop {
                match body.read(&mut buffer).await {
                    Ok(0) => break,
                    Ok(read) => tx.send(buffer[..read].to_vec()).await.expect("send"),
                    Err(error) => panic!("read: {error}"),
                }
            }
            tx.close();
        })
        .detach();

        let mut collected = Vec::new();
        loop {
            let next = smol::block_on(futures_lite::future::or(rx.recv(), async {
                smol::Timer::after(std::time::Duration::from_secs(3)).await;
                Err(smol::channel::RecvError)
            }));
            match next {
                Ok(chunk) => collected.extend_from_slice(&chunk),
                Err(_) => break,
            }
        }
        assert_eq!(collected, b"hello");
        server.join().expect("server");
    }

    /// Same again, but the main thread only sleeps: nothing on it drives the executor, which
    /// is what the realm thread does.
    #[test]
    fn one_request_while_the_main_thread_only_sleeps() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut buffer = [0u8; 4096];
            let _ = socket.read(&mut buffer);
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                )
                .expect("write");
            socket.flush().ok();
        });

        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            smol::block_on(async move {
                let request = Request {
                    method: "GET".to_string(),
                    url: format!("http://127.0.0.1:{port}/"),
                    headers: Vec::new(),
                    body: None,
                };
                let cancel = Arc::new(AtomicBool::new(false));
                let response = send(request, true, cancel).await.expect("response");
                let mut body = response.body;
                let mut buffer = vec![0u8; 16 * 1024];
                loop {
                    match body.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(read) => tx.send(buffer[..read].to_vec()).expect("send"),
                        Err(error) => panic!("read: {error}"),
                    }
                }
                tx.send(Vec::new()).ok();
            });
        });

        let mut collected = Vec::new();
        let mut rounds = 0;
        loop {
            match rx.recv_timeout(std::time::Duration::from_millis(200)) {
                Ok(chunk) if chunk.is_empty() => break,
                Ok(chunk) => collected.extend_from_slice(&chunk),
                Err(_) => {
                    rounds += 1;
                    if rounds > 20 {
                        panic!("the body never finished, collected {:?}", collected);
                    }
                }
            }
        }
        assert_eq!(collected, b"hello");
        server.join().expect("server");
    }

    /// The client on its own, with no engine in the way: does one request come back?
    #[test]
    fn one_request_against_a_canned_server() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut buffer = [0u8; 4096];
            let _ = socket.read(&mut buffer);
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                )
                .expect("write");
            socket.flush().ok();
        });

        let request = Request {
            method: "GET".to_string(),
            url: format!("http://127.0.0.1:{port}/"),
            headers: Vec::new(),
            body: None,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let response = smol::block_on(send(request, true, cancel)).expect("response");
        assert_eq!(response.status, 200);
        let body = smol::block_on(async {
            let mut body = response.body;
            let mut out = Vec::new();
            body.read_to_end(&mut out).await.expect("read");
            out
        });
        assert_eq!(body, b"hello");
        server.join().expect("server");
    }
}
