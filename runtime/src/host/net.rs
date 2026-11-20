//! Networking: connections and listening. Async goes through smol's reactor.
//!
//! listen_reuseport is settled right here: one listener per thread, each accepting on its own,
//! so the whole per-core stack (executor + realm + client pool) doesn't need to share a single lock.

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use futures_lite::future;
use socket2::{Domain, Protocol, Socket, Type};

pub use smol::net::{TcpListener, TcpStream};

#[derive(Clone, Copy, Default)]
pub struct Net;

impl Net {
    pub async fn connect(&self, addr: &str) -> io::Result<TcpStream> {
        TcpStream::connect(addr).await
    }

    /// Connections with a timeout. smol doesn't have one built in, so race the connection against a timer.
    pub async fn connect_timeout(&self, addr: &str, after: Duration) -> io::Result<TcpStream> {
        future::or(TcpStream::connect(addr), async move {
            smol::Timer::after(after).await;
            Err(io::Error::new(io::ErrorKind::TimedOut, "connect timed out"))
        })
        .await
    }

    pub fn listen(&self, addr: &str) -> io::Result<TcpListener> {
        let listener = std::net::TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        Ok(TcpListener::from(smol::Async::new(listener)?))
    }

    /// You can start N of them on the same address (one per thread), and the kernel handles distribution.
    /// On Linux it really distributes; on macOS it binds but distribution is uneven, so to measure the effect you have to go to Linux.
    pub fn listen_reuseport(&self, addr: &str) -> io::Result<TcpListener> {
        let addr: SocketAddr = addr
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "bad address"))?;
        let domain = if addr.is_ipv6() {
            Domain::IPV6
        } else {
            Domain::IPV4
        };
        let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
        socket.set_reuse_address(true)?;
        socket.set_reuse_port(true)?;
        socket.set_nonblocking(true)?;
        socket.bind(&addr.into())?;
        socket.listen(1024)?;
        let listener: std::net::TcpListener = socket.into();
        Ok(TcpListener::from(smol::Async::new(listener)?))
    }

    /// The actual address of a connection; look at this when measuring reuseport distribution.
    pub fn local_addr(listener: &TcpListener) -> io::Result<SocketAddr> {
        listener.local_addr()
    }
}
