// server: the JS half of the HTTP server.
//
// Rust parses the request with trillium and calls __serve_dispatch with the parts. This file builds
// a Request out of them, calls the app's handler and hands the answer back: __serve_respond for a
// body that is already in hand, __serve_stream plus __serve_chunk for one that is written as it is
// produced. Nothing here parses HTTP and nothing here touches a socket.
//
// The handler may answer synchronously or return a promise; either way the answer arrives the same
// way, so Rust has one path. A handler that answers before it returns is the fast path: the answer
// is already waiting when the dispatch call comes back, and the connection never awaits.

(function () {
  'use strict';

  const SEP = '\u0000';

  let handler = null;
  let env = Object.freeze({});

  // One abort controller per request. A handler reads request.signal, and this is what fires when
  // the client goes away, which is how a streaming handler learns to stop its own timers.
  const controls = new Map();

  // Anything the handler asked to finish after the answer went out.
  const flights = new Set();

  // A thrown value read back for a human: the engine's stack for an Error carries only the
  // frames, so the name and message are put in front of it rather than trusted to be there.
  const describe = (error) => {
    if (error === null || error === undefined) return String(error);
    const message = typeof error.message === 'string' ? error.message : '';
    const name = typeof error.name === 'string' ? error.name : '';
    const head = message === '' ? String(error) : name === '' ? message : name + ': ' + message;
    const stack = typeof error.stack === 'string' ? error.stack : '';
    if (stack === '') return head;
    if (message !== '' && stack.indexOf(message) >= 0) return stack;
    return head + '\n' + stack;
  };

  function pairs(text) {
    const parts = text.split(SEP);
    const out = [];
    for (let i = 0; i + 1 < parts.length; i += 2) {
      if (parts[i] !== '') out.push([parts[i], parts[i + 1]]);
    }
    return out;
  }

  // The request is over: nothing will fire abort any more.
  function done(id) {
    controls.delete(id);
  }

  // The client is gone. Firing abort is what lets a handler clean up; a handler that ignores it
  // is still not wrong, it just keeps its timers until it produces the next chunk.
  function giveUp(id, reason) {
    const control = controls.get(id);
    controls.delete(id);
    if (control === undefined) return;
    try {
      control.abort(reason);
    } catch (error) {
      // An abort listener that throws is the handler's business, not the server's.
    }
  }

  function waitUntil(value) {
    const tracked = Promise.resolve(value).then(
      () => {},
      (error) => {
        console.error('waitUntil task failed: ' + describe(error));
      },
    ).then(() => {
      flights.delete(tracked);
    });
    flights.add(tracked);
  }

  const context = { waitUntil };

  globalThis.__serve_dispatch = function (id, method, target, headersText, body) {
    if (handler === null) {
      __serve_fail(id, 'no handler: call Xmas.serve(handler) before serving');
      return;
    }

    const pairsIn = pairs(headersText);
    const headers = new Headers();
    let host = 'localhost';
    let hasBody = body !== null && body.byteLength > 0;
    for (let i = 0; i < pairsIn.length; i += 1) {
      const name = pairsIn[i][0];
      const value = pairsIn[i][1];
      if (name.toLowerCase() === 'host') host = value;
      try {
        headers.append(name, value);
      } catch (error) {
        // A header we refuse to repeat is not a reason to fail the request.
      }
    }

    const control = new AbortController();
    controls.set(id, control);

    let request;
    try {
      const url = target.startsWith('http://') || target.startsWith('https://')
        ? target
        : 'http://' + host + target;
      const init = { method, headers, signal: control.signal };
      if (hasBody && method !== 'GET' && method !== 'HEAD') init.body = body;
      request = new Request(url, init);
    } catch (error) {
      giveUp(id, 'the request could not be built');
      __serve_fail(id, describe(error));
      return;
    }

    let result;
    try {
      result = handler(request, env, context);
    } catch (error) {
      giveUp(id, 'the handler threw');
      __serve_fail(id, describe(error));
      return;
    }

    Promise.resolve(result).then(
      (response) => {
        answer(id, method, response);
      },
      (error) => {
        giveUp(id, 'the handler rejected');
        __serve_fail(id, describe(error));
      },
    );
  };

  function answer(id, method, response) {
    if (response === null || typeof response !== 'object') {
      giveUp(id, 'no answer');
      __serve_fail(id, 'handler returned ' + (response === null ? 'null' : typeof response) + ', not a Response');
      return;
    }
    if (typeof response.status !== 'number' || !response.headers) {
      giveUp(id, 'no answer');
      __serve_fail(id, 'handler returned an object that is not a Response');
      return;
    }

    const out = [];
    response.headers.forEach((value, name) => {
      out.push(name + SEP + value);
    });
    const headersText = out.join(SEP) + (out.length > 0 ? SEP : '');

    // An answer to a websocket handshake: the connection stops being HTTP once the 101 is out, and
    // the socket the handler made is already waiting for its frames.
    const upgrade = response._upgrade;
    if (upgrade !== undefined) {
      hand_over_upgrade(id, response, upgrade, headersText);
      return;
    }

    // A body that is a stream goes out as it is produced: the head first, then each chunk. This is
    // what server-sent events need, and it is also how a long response avoids being held in memory.
    const source = response._source === undefined ? null : response._source;
    if (source !== null && method !== 'HEAD') {
      stream(id, response, headersText, source);
      return;
    }

    let reading;
    try {
      reading = response.arrayBuffer();
    } catch (error) {
      giveUp(id, 'the body could not be read');
      __serve_fail(id, 'reading the response body failed: ' + describe(error));
      return;
    }
    Promise.resolve(reading).then(
      (buffer) => {
        const body = method === 'HEAD' ? new Uint8Array(0) : new Uint8Array(buffer);
        __serve_respond(id, response.status, response.statusText || '', headersText, body);
        done(id);
      },
      (error) => {
        giveUp(id, 'the body could not be read');
        __serve_fail(id, 'reading the response body failed: ' + describe(error));
      },
    );
  }

  function hand_over_upgrade(id, response, socket, headersText) {
    if (__serve_upgrade(id, response.status, response.statusText || '', headersText) !== true) {
      // The connection was already gone, so the socket has nobody to talk to.
      socket._failed('the client went away before the handshake');
      return;
    }
    done(id);
  }

  function stream(id, response, headersText, source) {
    if (__serve_stream(id, response.status, response.statusText || '', headersText) === false) {
      giveUp(id, 'the client went away');
      return;
    }

    let reader;
    try {
      reader = source.getReader();
    } catch (error) {
      __serve_end(id);
      giveUp(id, 'the body could not be read');
      __serve_fail(id, 'reading the response body failed: ' + describe(error));
      return;
    }

    const step = () => {
      reader.read().then(
        (result) => {
          if (result.done === true) {
            __serve_end(id);
            done(id);
            return;
          }
          let bytes;
          try {
            const value = result.value;
            bytes = value instanceof Uint8Array ? value : new Uint8Array(value);
          } catch (error) {
            __serve_end(id);
            giveUp(id, 'the body produced something that is not bytes');
            return;
          }
          send(bytes);
        },
        (error) => {
          __serve_end(id);
          giveUp(id, 'the body failed');
          __serve_fail(id, 'the response body failed: ' + describe(error));
        },
      );
    };

    // One chunk at a time: the next read waits until this one is on the connection, so a fast
    // producer is slowed down by a slow client instead of filling memory.
    const send = (bytes) => {
      __serve_chunk(id, bytes).then(
        (accepted) => {
          if (accepted !== true) {
            __serve_end(id);
            giveUp(id, 'the client went away');
            return;
          }
          step();
        },
        (error) => {
          __serve_end(id);
          giveUp(id, 'the body could not be written');
          __serve_fail(id, 'writing the response body failed: ' + describe(error));
        },
      );
    };

    step();
  }

  function serve(target) {
    let fn = target;
    if (target !== null && typeof target === 'object') {
      fn = target.fetch;
      // The environment is the caller's, not the platform's: whatever serve was given is handed to
      // the handler unchanged, and an empty frozen object when it was given nothing.
      if (target.env !== undefined) env = target.env;
      // The listener is already up by the time the entry runs, so a port asked for here can only be
      // compared with the one it has. A silent mismatch would look like the app is on the wrong port.
      checkAddress(target.port, target.hostname);
    }
    if (typeof fn !== 'function') {
      throw new TypeError('Xmas.serve needs a function or an object with a fetch method');
    }
    handler = fn;
    return { fetch: fn };
  }

  // The address this realm's server is listening on, put there before the entry ran. Absent when the
  // realm was built by a test or an embedder with no listener.
  function checkAddress(port, hostname) {
    const addr = globalThis.__serve_addr;
    if (addr === undefined || addr === null) return;
    const at = addr.lastIndexOf(':');
    const bound = Number(addr.slice(at + 1));
    if (port !== undefined && Number(port) !== bound) {
      throw new Error(
        'the entry asked for port ' + port + ' but this server listens on ' + bound +
        ' (pass --port ' + port + ' to move it)',
      );
    }
    if (hostname !== undefined) {
      const bound_host = addr.slice(0, at);
      // A wildcard bind answers on every address, so any name it covers is the same server.
      if (bound_host === '0.0.0.0' || bound_host === '::' || bound_host === '[::]') return;
      if (String(hostname) !== bound_host) {
        throw new Error(
          'the entry asked for hostname ' + hostname + ' but this server listens on ' +
          bound_host + ' (pass --host ' + hostname + ' to move it)',
        );
      }
    }
  }

  // An entry module's handler: its own fetch, then the fetch on its default export. Either shape
  // works, and an entry that called Xmas.serve at the top level has already registered a handler
  // that this never replaces.
  function takeEntry(namespace) {
    if (handler !== null) return;
    if (namespace !== null && namespace !== undefined && typeof namespace === 'object') {
      if (typeof namespace.fetch === 'function') {
        handler = namespace.fetch;
        return;
      }
      const named = namespace.default;
      if (named !== null && (typeof named === 'object' || typeof named === 'function')) {
        if (typeof named.fetch === 'function') {
          handler = named.fetch;
          return;
        }
      }
    }
    throw new TypeError(
      'the entry has no fetch handler: export default { fetch }, export fetch, or call Xmas.serve(handler)',
    );
  }

  globalThis.Xmas = { serve };
  globalThis.__serve_entry = takeEntry;
  globalThis.__serve_module = { serve };
})();
