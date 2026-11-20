//! HTTP server: trillium's HTTP/1.1 machinery on an accept loop we own.
//!
//! trillium_smol::config().run(...) would serve HTTP too, but it hands every connection to the
//! runtime's spawn, and outside a Linux reuseport worker that is the multi-threaded global
//! executor. A connection future can then migrate between threads, and the realm cannot follow it:
//! the realm is an Rc over a single-threaded engine. HttpContext::run puts no Send bound on the
//! handler future, so we own the thread instead: one SO_REUSEPORT listener and one
//! single-threaded executor per worker, so the realm of a worker and the connections it serves
//! live on the same thread and a request costs no thread hop.
//!
//! Everything HTTP lives in trillium: the request head parser, chunked bodies, keep-alive,
//! pipelining, trailers, 100-continue and the graceful close. This file only decides which thread
//! runs it.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::thread::JoinHandle;

use futures_lite::future;
use futures_lite::stream::Stream as _;
use smol::LocalExecutor;
use trillium::Conn;
use trillium_http::HttpContext;
use trillium_smol::SmolTransport;

use crate::host::net::{Net, TcpListener, TcpStream};

/// What a connection is carried on: smol's socket, wrapped so trillium can name its peer.
pub type Transport = SmolTransport<TcpStream>;

/// One answer from JS, in the flat shape a connection needs: no engine values, so the connection
/// future can hold it while it does socket work.
pub struct Answer {
    pub status: u16,
    pub status_text: String,
    /// Response headers as a NUL-joined name/value list, which is how pairs cross this boundary.
    pub headers: String,
    pub body: AnswerBody,
}

/// The body of an answer, either in hand or still arriving from the realm.
pub enum AnswerBody {
    /// The whole body, so trillium frames it with its length.
    Bytes(Vec<u8>),
    /// A body written after the head has gone out, framed chunked as it arrives. This is what
    /// server-sent events need: the client sees each chunk when the handler produces it, not when
    /// the handler is done.
    Stream(ChunkReader),
}

/// How many chunks of a streaming body may be queued ahead of the connection. A handler that
/// ignores backpressure fills this and then waits for the socket instead of holding the whole
/// response in memory.
const STREAM_CHUNKS: usize = 4;

/// Reads a streaming response body out of the channel the realm writes into.
///
/// The end of the body is the sender being dropped, which is what __serve_end does; the channel
/// being closed that way is also how a connection that went away shows up on the writing side.
pub struct ChunkReader {
    // Boxed and pinned because the channel's receiver is deliberately not Unpin.
    chunks: Pin<Box<smol::channel::Receiver<Vec<u8>>>>,
    current: Vec<u8>,
    offset: usize,
}

impl futures_lite::AsyncRead for ChunkReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            if self.offset < self.current.len() {
                let take = (self.current.len() - self.offset).min(buf.len());
                buf[..take].copy_from_slice(&self.current[self.offset..self.offset + take]);
                self.offset += take;
                return Poll::Ready(Ok(take));
            }
            if buf.is_empty() {
                return Poll::Ready(Ok(0));
            }
            match self.chunks.as_mut().poll_next(cx) {
                Poll::Ready(Some(chunk)) => {
                    self.current = chunk;
                    self.offset = 0;
                }
                Poll::Ready(None) => return Poll::Ready(Ok(0)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// The realm side of a server: which request is waiting for which answer.
///
/// The JS side names a request by the id it was given, so an answer that arrives long after the
/// call returned still finds its connection. A request whose answer has already arrived is found
/// in the map and sent immediately, which is what makes a synchronous handler cost no await.
#[derive(Default)]
pub struct State {
    next: Cell<u64>,
    waiting: RefCell<BTreeMap<u64, smol::channel::Sender<Result<Answer, String>>>>,
    /// The realm's request dispatcher, installed with the server module. Looked up once so a
    /// request never pays for a property lookup.
    pub dispatch: RefCell<Option<quickjs_oxide::engine::api::core::CallableRef>>,
    /// Streaming answers in flight: the end the realm writes chunks into, by request id.
    streams: RefCell<BTreeMap<u64, smol::channel::Sender<Vec<u8>>>>,
}

impl State {
    /// Reserve a slot for one request. The id is what JS hands back to __serve_respond.
    pub fn open(&self) -> (u64, smol::channel::Receiver<Result<Answer, String>>) {
        let id = self.next.get();
        self.next.set(id + 1);
        let (sender, receiver) = smol::channel::bounded(1);
        self.waiting.borrow_mut().insert(id, sender);
        (id, receiver)
    }

    /// Deliver an answer. Returns false when nothing is waiting under that id, which means the
    /// connection is already gone and the answer has nowhere to land.
    pub fn settle(&self, id: u64, outcome: Result<Answer, String>) -> bool {
        match self.waiting.borrow_mut().remove(&id) {
            Some(sender) => sender.try_send(outcome).is_ok(),
            None => false,
        }
    }

    /// Forget a slot: the connection gave up before the answer arrived.
    pub fn close(&self, id: u64) {
        self.waiting.borrow_mut().remove(&id);
        self.streams.borrow_mut().remove(&id);
    }

    /// Forget only the slot. A streaming answer outlives the slot that carried its head: the body
    /// is still being written, so its channel has to stay open until it ends.
    pub fn forget(&self, id: u64) {
        self.waiting.borrow_mut().remove(&id);
    }

    /// Start a streaming answer. The head is settled by the caller; the body is written into the
    /// returned sender, and the reader is what trillium frames onto the connection.
    pub fn open_stream(&self, id: u64) -> (smol::channel::Sender<Vec<u8>>, ChunkReader) {
        let (sender, receiver) = smol::channel::bounded(STREAM_CHUNKS);
        self.streams.borrow_mut().insert(id, sender.clone());
        (
            sender,
            ChunkReader {
                chunks: Box::pin(receiver),
                current: Vec::new(),
                offset: 0,
            },
        )
    }

    /// The writing end of a streaming answer, if it is still open.
    pub fn stream(&self, id: u64) -> Option<smol::channel::Sender<Vec<u8>>> {
        self.streams.borrow().get(&id).cloned()
    }

    /// End a streaming body: dropping the sender is what the reader sees as the end.
    pub fn end_stream(&self, id: u64) {
        self.streams.borrow_mut().remove(&id);
    }
}

/// A running server. Dropping it stops accepting and drops the connections in flight.
pub struct Server {
    addr: SocketAddr,
    stop: Vec<smol::channel::Sender<()>>,
    threads: Vec<JoinHandle<()>>,
}

impl Server {
    /// The address actually bound, which is how a caller discovers a port it asked for as 0.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stop accepting and wait for the worker threads. A connection that is still open is dropped
    /// with its worker, which is what a test wants; a real deployment drains with a deadline.
    pub fn stop(&mut self) {
        for stop in self.stop.drain(..) {
            let _ = stop.try_send(());
        }
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Serve on addr with one worker thread per core, each with its own listener and its own handler.
///
/// make is called once per worker, on that worker's own thread, so whatever the handler holds is
/// built where it will run. A worker binds its own SO_REUSEPORT listener into the same group, so
/// the kernel decides which worker gets a connection and no userspace hand-off is needed.
///
/// On a platform where SO_REUSEPORT does not distribute connections the extra workers still work,
/// they just do not share the load as evenly; one worker is the safe choice there.
pub fn serve<F, H, Fut>(addr: &str, workers: usize, make: F) -> io::Result<Server>
where
    F: Fn(usize, &'static LocalExecutor) -> H + Send + Sync + Clone + 'static,
    H: Fn(Conn) -> Fut + 'static,
    Fut: Future<Output = Conn> + 'static,
{
    let workers = workers.max(1);
    // Bind here so a port of 0 resolves once, and so a bad address fails in the caller rather than
    // in a worker thread nobody is watching. One worker needs no reuseport group, and asking for
    // one on port 0 lets two servers land on the same port, which is a test's worst nightmare.
    let mut prebound = Some(if workers == 1 {
        Net.listen(addr)?
    } else {
        Net.listen_reuseport(addr)?
    });
    let bound = Net::local_addr(prebound.as_ref().expect("just bound"))?;

    let mut stop_channels = Vec::with_capacity(workers);
    let mut threads = Vec::with_capacity(workers);

    for index in 0..workers {
        let own = if index == 0 { prebound.take() } else { None };
        let (stop, stopped) = smol::channel::unbounded::<()>();
        let make = make.clone();
        let name = format!("xmas-http-{index}");
        let thread = std::thread::Builder::new().name(name).spawn(move || {
            let listener = match own {
                Some(listener) => listener,
                None => match Net.listen_reuseport(&bound.to_string()) {
                    Ok(listener) => listener,
                    Err(error) => {
                        eprintln!("worker {index} could not bind {bound}: {error}");
                        return;
                    }
                },
            };
            work(listener, make, index, stopped);
        })?;
        stop_channels.push(stop);
        threads.push(thread);
    }

    Ok(Server {
        addr: bound,
        stop: stop_channels,
        threads,
    })
}

/// One worker: accept on this thread, run the connections on this thread.
fn work<F, H, Fut>(listener: TcpListener, make: F, index: usize, stopped: smol::channel::Receiver<()>)
where
    F: Fn(usize, &'static LocalExecutor) -> H,
    H: Fn(Conn) -> Fut + 'static,
    Fut: Future<Output = Conn> + 'static,
{
    // One executor per worker, leaked because it lives as long as the thread does. Connection
    // futures are spawned here and are never required to be Send.
    let executor: &'static LocalExecutor = Box::leak(Box::new(LocalExecutor::new()));
    let context = Arc::new(HttpContext::new());
    // The handler is built here, on this thread, and gets the executor so it can spawn the pump
    // that keeps its realm turning between requests.
    let handler = Rc::new(make(index, executor));

    smol::block_on(executor.run(async move {
        loop {
            let accepted = future::or(
                async { Some(listener.accept().await) },
                async {
                    stopped.recv().await.ok();
                    None
                },
            )
            .await;

            let Some(accepted) = accepted else {
                return;
            };

            match accepted {
                Ok((stream, _peer)) => {
                    let context = Arc::clone(&context);
                    let handler = Rc::clone(&handler);
                    executor
                        .spawn(async move { connection(stream, context, handler).await })
                        .detach();
                }
                // A connection that dies during the handshake is the peer's business, not ours.
                Err(error) if error.kind() == io::ErrorKind::ConnectionAborted => {}
                Err(error) => {
                    // A listener error that keeps repeating would spin, so give up on this worker.
                    eprintln!("accept failed: {error}");
                    return;
                }
            }
        }
    }));
}

/// One connection, from the first byte to whatever ends it.
async fn connection<H, Fut>(stream: TcpStream, context: Arc<HttpContext>, handler: Rc<H>)
where
    H: Fn(Conn) -> Fut + 'static,
    Fut: Future<Output = Conn> + 'static,
{
    let transport = SmolTransport::from(stream);
    let served = context
        .run(transport, move |http| {
            let conn = Conn::from(http);
            let answered = handler.as_ref()(conn);
            async move { answered.await.into_inner() }
        })
        .await;
    if let Err(error) = served {
        // A malformed request lands here, and so does a peer that vanished mid-request. The second
        // kind is ordinary (a closed tab, a failed load test) and does not deserve a log line.
        let quiet = match &error {
            trillium_http::Error::Io(io) => matches!(
                io.kind(),
                io::ErrorKind::BrokenPipe
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::UnexpectedEof
                    | io::ErrorKind::WouldBlock
            ),
            trillium_http::Error::Closed => true,
            _ => false,
        };
        if !quiet {
            eprintln!("http error: {error:?}");
        }
    }
}
