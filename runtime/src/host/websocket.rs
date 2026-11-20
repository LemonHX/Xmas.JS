//! WebSocket over an upgraded connection: the frame codec, and the loop that runs it.
//!
//! trillium hands over an Upgrade once the 101 head has gone out. That is a plain byte stream with
//! no framing at all, so everything WebSocket lives here: the accept value for the handshake, the
//! frame header, masking (a client masks, a server does not), fragmentation, and the ping, pong and
//! close control frames.
//!
//! The loop runs on the worker thread next to the realm, so that a message never has to cross a
//! thread to reach JS: it goes into a queue that __ws_read pulls from. The loop stops reading the
//! socket while that queue is full, which is the only backpressure a socket gives you.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io;
use std::rc::Rc;

use futures_lite::{AsyncReadExt, AsyncWriteExt};
use trillium_http::Upgrade;

use crate::host::pending::{Job, Payload};
use crate::host::server::Transport;

/// Not a wire opcode: the handshake is done, which is the JS side's cue to fire open.
pub const OP_OPEN: u8 = 0x00;
const OP_CONTINUATION: u8 = 0x00;
const OP_TEXT: u8 = 0x01;
const OP_BINARY: u8 = 0x02;
const OP_CLOSE: u8 = 0x08;
const OP_PING: u8 = 0x09;
const OP_PONG: u8 = 0x0A;

/// How many messages may wait for the JS side before the socket stops being read.
const QUEUE: usize = 32;
/// A message bigger than this is refused rather than buffered; a peer can send a length and then
/// take its time, and nothing here should be talked into an arbitrary allocation.
const MAX_MESSAGE: usize = 8 << 20;

/// The accept value of the handshake: base64 of sha1 over the client's key and the fixed GUID.
pub fn accept_for(key: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let digest = hasher.finalize();
    crate::modules::buffer::base64_encode(&digest)
}

/// One socket, seen from the realm side: the queue JS pulls from, and the queue it pushes into.
pub struct Ws {
    pub accept: String,
    inbound: RefCell<VecDeque<Vec<u8>>>,
    waiting: RefCell<Option<Job>>,
    outbox: smol::channel::Sender<Vec<u8>>,
    room: smol::channel::Sender<()>,
    ended: Cell<bool>,
}

/// The ends the frame loop holds. They are taken out of the state before the loop starts, so the
/// realm side cannot move them anywhere.
pub struct Pipes {
    outgoing: smol::channel::Receiver<Vec<u8>>,
    roomed: smol::channel::Receiver<()>,
}

impl Ws {
    /// A socket waiting for its connection, plus the ends the frame loop will take.
    pub fn new(accept: String) -> (Rc<Ws>, Pipes) {
        // Unbounded: a handler that sends faster than the wire takes it buffers here. The other
        // end of a JS send would be a promise, and deno's socket.send is not one.
        let (outbox, outgoing) = smol::channel::unbounded();
        let (room, roomed) = smol::channel::bounded(1);
        let ws = Rc::new(Ws {
            accept,
            inbound: RefCell::new(VecDeque::new()),
            waiting: RefCell::new(None),
            outbox,
            room,
            ended: Cell::new(false),
        });
        (ws, Pipes { outgoing, roomed })
    }

    /// Hand a message to the JS side: straight to a parked read, or to the queue.
    pub fn deliver(&self, message: Vec<u8>) {
        match self.waiting.borrow_mut().take() {
            Some(job) => job.finish(Ok(Payload::Bytes(message))),
            None => self.inbound.borrow_mut().push_back(message),
        }
    }

    /// One message for the JS side, if there is one waiting.
    pub fn pull(&self) -> Option<Vec<u8>> {
        let message = self.inbound.borrow_mut().pop_front();
        if message.is_some() {
            // Reading made room, so the frame loop can take the socket again.
            let _ = self.room.try_send(());
        }
        message
    }

    /// The frame loop is finished: no more messages are coming, and a parked read has to hear it.
    pub fn end(&self) {
        self.ended.set(true);
        if let Some(job) = self.waiting.borrow_mut().take() {
            job.finish(Ok(Payload::Text("closed".to_string())));
        }
    }

    /// True once the connection is gone, or once a read parked and was answered "closed".
    pub fn ended(&self) -> bool {
        self.ended.get()
    }

    pub fn queued(&self) -> usize {
        self.inbound.borrow().len()
    }

    pub fn park(&self, job: Job) {
        *self.waiting.borrow_mut() = Some(job);
    }

    pub fn waiting(&self) -> bool {
        self.waiting.borrow().is_some()
    }

    /// Queue a message for the wire. False when the connection is already gone.
    pub fn send(&self, message: Vec<u8>) -> bool {
        self.outbox.try_send(message).is_ok()
    }
}

/// Run one upgraded connection until it ends.
pub async fn serve(ws: Rc<Ws>, pipes: Pipes, mut upgrade: Upgrade<Transport>) {
    // The handshake went out; the JS side fires open when it sees this.
    ws.deliver(vec![OP_OPEN]);

    let mut reader = Reader::default();
    let mut outgoing: VecDeque<Vec<u8>> = VecDeque::new();
    let mut assembling: Option<(u8, Vec<u8>)> = None;
    let mut closing = false;

    loop {
        // Everything JS asked to write, in order, each frame written whole.
        loop {
            let next = match outgoing.pop_front() {
                Some(message) => Some(message),
                None => pipes.outgoing.try_recv().ok(),
            };
            let Some(message) = next else { break };
            if message.first() == Some(&OP_CLOSE) {
                closing = true;
            }
            if write_frame(&mut upgrade, &message).await.is_err() {
                ws.end();
                return;
            }
        }
        if upgrade.flush().await.is_err() {
            ws.end();
            return;
        }

        // Stop reading while the JS side is behind; the socket is the only place left to push back.
        while ws.queued() >= QUEUE && !ws.waiting() {
            if pipes.roomed.recv().await.is_err() {
                ws.end();
                return;
            }
        }

        let next = futures_lite::future::or(
            async { reader.frame(&mut upgrade).await.map(Next::Frame) },
            async { pipes.outgoing.recv().await.ok().map(Next::Out) },
        )
        .await;

        match next {
            None => {
                ws.end();
                return;
            }
            Some(Next::Out(message)) => outgoing.push_back(message),
            Some(Next::Frame(frame)) => {
                match frame.opcode {
                    OP_TEXT | OP_BINARY => {
                        if frame.payload.len() > MAX_MESSAGE {
                            ws.end();
                            return;
                        }
                        if frame.fin {
                            ws.deliver(with_opcode(frame.opcode, frame.payload));
                        } else {
                            assembling = Some((frame.opcode, frame.payload));
                        }
                    }
                    OP_CONTINUATION => {
                        let Some((opcode, mut payload)) = assembling.take() else {
                            // A continuation with nothing to continue is a protocol error.
                            ws.end();
                            return;
                        };
                        if payload.len() + frame.payload.len() > MAX_MESSAGE {
                            ws.end();
                            return;
                        }
                        payload.extend_from_slice(&frame.payload);
                        if frame.fin {
                            ws.deliver(with_opcode(opcode, payload));
                        } else {
                            assembling = Some((opcode, payload));
                        }
                    }
                    OP_PING => {
                        outgoing.push_back(with_opcode(OP_PONG, frame.payload));
                    }
                    OP_PONG => {}
                    OP_CLOSE => {
                        // Echo the code back, hand the close to JS, and let the connection go.
                        outgoing.push_back(with_opcode(OP_CLOSE, frame.payload.clone()));
                        ws.deliver(with_opcode(OP_CLOSE, frame.payload));
                        while let Some(message) = outgoing.pop_front() {
                            let _ = write_frame(&mut upgrade, &message).await;
                        }
                        let _ = upgrade.flush().await;
                        ws.end();
                        return;
                    }
                    _ => {
                        // An opcode nobody defined: nothing sensible to do with it.
                        ws.end();
                        return;
                    }
                }
            }
        }

        if closing && ws.queued() == 0 {
            // The close frame is on the wire and there is nothing left to say.
            ws.end();
            return;
        }
    }
}

enum Next {
    Frame(Frame),
    Out(Vec<u8>),
}

fn with_opcode(opcode: u8, payload: Vec<u8>) -> Vec<u8> {
    let mut message = Vec::with_capacity(payload.len() + 1);
    message.push(opcode);
    message.extend_from_slice(&payload);
    message
}

struct Frame {
    fin: bool,
    opcode: u8,
    payload: Vec<u8>,
}

/// Reads one frame at a time, keeping everything it has already taken off the socket.
///
/// The buffered form is what makes this safe to cancel: this read is raced against the outgoing
/// queue, and a race that ends in the other branch drops the future mid-frame. Bytes are appended
/// to the buffer before the next await, so a dropped future loses nothing and the next call carries
/// on where this one stopped.
#[derive(Default)]
struct Reader {
    buf: Vec<u8>,
    pos: usize,
}

impl Reader {
    async fn frame<S: futures_lite::AsyncRead + Unpin>(&mut self, stream: &mut S) -> Option<Frame> {
        // Two bytes say how long the rest of the header is.
        self.need(stream, 2).await?;
        let b0 = self.buf[self.pos];
        let b1 = self.buf[self.pos + 1];
        let masked = b1 & 0x80 != 0;
        let short = (b1 & 0x7f) as usize;
        let extra = match short {
            126 => 2,
            127 => 8,
            _ => 0,
        };
        let head = 2 + extra + if masked { 4 } else { 0 };
        self.need(stream, head).await?;

        let start = self.pos;
        let length = match extra {
            0 => short,
            2 => {
                let mut bytes = [0u8; 2];
                bytes.copy_from_slice(&self.buf[start + 2..start + 4]);
                u16::from_be_bytes(bytes) as usize
            }
            _ => {
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&self.buf[start + 2..start + 10]);
                u64::from_be_bytes(bytes) as usize
            }
        };
        // Nothing here may be talked into an allocation by a length a peer made up.
        if length > MAX_MESSAGE {
            return None;
        }

        self.need(stream, head + length).await?;
        let mut payload = self.buf[start + head..start + head + length].to_vec();
        if masked {
            let mut key = [0u8; 4];
            key.copy_from_slice(&self.buf[start + 2 + extra..start + 2 + extra + 4]);
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= key[index % 4];
            }
        }

        self.pos = start + head + length;
        // Keep the buffer from growing forever on a long connection.
        if self.pos > 1 << 16 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }

        Some(Frame {
            fin: b0 & 0x80 != 0,
            opcode: b0 & 0x0f,
            payload,
        })
    }

    /// Fill the buffer until it holds n bytes from the read point.
    async fn need<S: futures_lite::AsyncRead + Unpin>(
        &mut self,
        stream: &mut S,
        n: usize,
    ) -> Option<()> {
        while self.buf.len() - self.pos < n {
            let mut chunk = [0u8; 8192];
            let read = stream.read(&mut chunk).await.ok()?;
            if read == 0 {
                return None;
            }
            self.buf.extend_from_slice(&chunk[..read]);
        }
        Some(())
    }
}

/// One frame out: the server never masks, and every frame it sends is final.
async fn write_frame<S: AsyncWriteExt + Unpin>(stream: &mut S, message: &[u8]) -> io::Result<()> {
    let Some((&opcode, payload)) = message.split_first() else {
        return Ok(());
    };
    let mut head = Vec::with_capacity(10);
    head.push(0x80 | opcode);
    let len = payload.len();
    if len < 126 {
        head.push(len as u8);
    } else if len <= u16::MAX as usize {
        head.push(126);
        head.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        head.push(127);
        head.extend_from_slice(&(len as u64).to_be_bytes());
    }
    stream.write_all(&head).await?;
    stream.write_all(payload).await
}
