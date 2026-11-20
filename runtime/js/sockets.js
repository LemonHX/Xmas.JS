// sockets: connect() from the WinterCG sockets proposal, spelled the way cloudflare:sockets
// spells it. A socket is one id behind two web streams, so a read is a pull and a write is a
// write; nothing here reads ahead of the caller or buffers what nobody asked for.

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

globalThis.__sockets_module = { connect, default: { connect } };
