// sockets: connect() from the WinterCG sockets proposal, spelled the way cloudflare:sockets
// spells it, plus UDPSocket for the datagram side of the same idea. A socket is one id behind two
// web streams, so a read is a pull and a write is a write; nothing here reads ahead of the caller
// or buffers what nobody asked for.
//
// The one thing UDP changes: the readable side is a stream of datagrams, so the edges matter. One
// enqueued chunk is one datagram, exactly as it arrived, and an empty datagram is still a
// datagram rather than an end of stream.

let nextSocket = 1;
const EOF = 'eof';

const typeError = (message) => new TypeError(message);

const readAddress = (address) => {
  if (address === null || typeof address !== 'object') {
    throw typeError('connect() takes an object with hostname and port');
  }
  const hostname = address.hostname;
  if (typeof hostname !== 'string' || hostname.length === 0) {
    throw typeError('hostname must be a non-empty string');
  }
  const port = address.port;
  if (typeof port !== 'number' || !Number.isInteger(port) || port < 1 || port > 65535) {
    throw typeError('port must be an integer between 1 and 65535');
  }
  return hostname;
};

const connect = (address, options) => {
  const hostname = readAddress(address);
  const port = address.port;
  if (options !== undefined && options !== null) {
    const transport = options.secureTransport;
    if (transport !== undefined && transport !== 'off') {
      throw typeError('secureTransport is not supported: this socket is plain TCP');
    }
  }

  const id = nextSocket;
  nextSocket += 1;

  let running = true;
  let resolveClosed;
  const closed = new Promise((resolve) => {
    resolveClosed = resolve;
  });
  const finish = () => {
    if (!running) {
      return;
    }
    running = false;
    resolveClosed();
  };

  const opened = __sockets_connect(id, hostname, port).then((text) => {
    const parts = String(text).split('\0');
    return { remoteAddress: parts[0], localAddress: parts[1] };
  });
  // A connect that fails is reported on opened, and the streams have nothing behind them. Handle
  // it here so a caller who only uses the socket does not also get an unhandled rejection.
  opened.catch(() => {
    finish();
    __sockets_close(id);
  });
  // A write or a read may be issued before the connection is up; the handle only exists once it
  // is, so both wait here rather than failing with "the socket is closed" for a socket that is
  // still being opened.
  const ready = opened.then(
    () => undefined,
    () => undefined,
  );

  const readable = new ReadableStream(
    {
      async pull(controller) {
        if (!running) {
          controller.close();
          return;
        }
        await ready;
        const chunk = await __sockets_read(id);
        if (typeof chunk === 'string') {
          controller.close();
          finish();
          __sockets_close(id);
          return;
        }
        controller.enqueue(new Uint8Array(chunk));
      },
      cancel() {
        finish();
        __sockets_close(id);
      },
    },
    { highWaterMark: 0 },
  );

  const writable = new WritableStream(
    {
      async write(chunk) {
        await ready;
        if (!running) {
          throw typeError('the socket is closed');
        }
        await __sockets_write(id, chunk);
      },
      async close() {
        if (!running) {
          return;
        }
        // A half close: the other side sees the end of the body, and whatever it still sends is
        // readable here.
        __sockets_end(id);
      },
      async abort() {
        finish();
        __sockets_close(id);
      },
    },
    { highWaterMark: 0 },
  );

  return {
    readable,
    writable,
    opened,
    closed,
    close() {
      finish();
      __sockets_close(id);
      return Promise.resolve();
    },
    startTls() {
      return Promise.reject(typeError('startTls is not supported'));
    },
  };
};

const openDatagram = (options) => {
  const remoteAddress = options.remoteAddress;
  if (typeof remoteAddress !== 'string' || remoteAddress.length === 0) {
    throw typeError('remoteAddress must be a non-empty string');
  }
  const remotePort = options.remotePort;
  if (
    typeof remotePort !== 'number' ||
    !Number.isInteger(remotePort) ||
    remotePort < 1 ||
    remotePort > 65535
  ) {
    throw typeError('remotePort must be an integer between 1 and 65535');
  }
  const localAddress = options.localAddress === undefined ? '' : options.localAddress;
  if (typeof localAddress !== 'string') {
    throw typeError('localAddress must be a string');
  }
  const localPort = options.localPort === undefined ? 0 : options.localPort;
  if (
    typeof localPort !== 'number' ||
    !Number.isInteger(localPort) ||
    localPort < 0 ||
    localPort > 65535
  ) {
    throw typeError('localPort must be an integer between 0 and 65535');
  }

  const id = nextSocket;
  nextSocket += 1;

  let running = true;
  let resolveClosed;
  const closed = new Promise((resolve) => {
    resolveClosed = resolve;
  });
  const finish = () => {
    if (!running) {
      return;
    }
    running = false;
    resolveClosed();
  };

  const opened = __udp_open(id, remoteAddress, remotePort, localAddress, localPort).then((text) => {
    const parts = String(text).split('\0');
    return { localAddress: parts[0], localPort: Number(parts[1]) };
  });
  opened.catch(() => {
    finish();
    __udp_close(id);
  });
  const ready = opened.then(
    () => undefined,
    () => undefined,
  );

  const readable = new ReadableStream(
    {
      async pull(controller) {
        if (!running) {
          controller.close();
          return;
        }
        await ready;
        const datagram = await __udp_recv(id);
        if (typeof datagram === 'string') {
          controller.close();
          finish();
          __udp_close(id);
          return;
        }
        // One datagram, whole: the buffer becomes the chunk as it is, no reassembly and no
        // coalescing with whatever came before it.
        controller.enqueue(new Uint8Array(datagram));
      },
      cancel() {
        finish();
        __udp_close(id);
      },
    },
    { highWaterMark: 0 },
  );

  const writable = new WritableStream(
    {
      async write(chunk) {
        await ready;
        if (!running) {
          throw typeError('the socket is closed');
        }
        // One write, one datagram. A chunk over 65507 bytes is refused by the host side rather
        // than split, because splitting would invent message boundaries the caller did not ask
        // for.
        await __udp_send(id, chunk);
      },
      async close() {
        // Nothing to say: a datagram socket has no end of stream to send.
      },
      async abort() {
        finish();
        __udp_close(id);
      },
    },
    { highWaterMark: 0 },
  );

  return {
    readable,
    writable,
    opened,
    closed,
    close() {
      finish();
      __udp_close(id);
      return Promise.resolve();
    },
  };
};

class UDPSocket {
  constructor(options) {
    if (options === null || typeof options !== 'object') {
      throw typeError('UDPSocket takes an object with remoteAddress and remotePort');
    }
    const socket = openDatagram(options);
    this.readable = socket.readable;
    this.writable = socket.writable;
    this.opened = socket.opened;
    this.closed = socket.closed;
    this.close = () => socket.close();
  }
}

globalThis.__sockets_module = { connect, UDPSocket, default: { connect, UDPSocket } };
