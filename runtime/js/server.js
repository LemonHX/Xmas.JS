// server: the JS half of the HTTP server.
//
// Rust parses the request with trillium, then calls __serve_dispatch with the parts. This file
// builds a Request out of them, calls the app's handler and hands the answer back through
// __serve_respond. Nothing here parses HTTP and nothing here touches a socket.
//
// The handler may answer synchronously or return a promise; either way the answer arrives the
// same way, so Rust has one path. A handler that answers before it returns is the fast path: the
// answer is already waiting when the dispatch call comes back, and the connection never awaits.

(function () {
  'use strict';

  const SEP = '\u0000';

  let handler = null;

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

  globalThis.__serve_dispatch = function (id, method, target, headersText, body) {
    if (handler === null) {
      __serve_fail(id, 'no handler: call Eta.serve(handler) before serving');
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

    let request;
    try {
      const url = target.startsWith('http://') || target.startsWith('https://')
        ? target
        : 'http://' + host + target;
      const init = { method, headers };
      if (hasBody && method !== 'GET' && method !== 'HEAD') init.body = body;
      request = new Request(url, init);
    } catch (error) {
      __serve_fail(id, describe(error));
      return;
    }

    let result;
    try {
      result = handler(request);
    } catch (error) {
      __serve_fail(id, describe(error));
      return;
    }

    Promise.resolve(result).then(
      (response) => {
        answer(id, method, response);
      },
      (error) => {
        __serve_fail(id, describe(error));
      },
    );
  };

  function answer(id, method, response) {
    if (response === null || typeof response !== 'object') {
      __serve_fail(id, 'handler returned ' + (response === null ? 'null' : typeof response) + ', not a Response');
      return;
    }
    if (typeof response.status !== 'number' || !response.headers) {
      __serve_fail(id, 'handler returned an object that is not a Response');
      return;
    }

    const out = [];
    response.headers.forEach((value, name) => {
      out.push(name + SEP + value);
    });
    const headersText = out.join(SEP) + (out.length > 0 ? SEP : '');

    let reading;
    try {
      reading = response.arrayBuffer();
    } catch (error) {
      __serve_fail(id, 'reading the response body failed: ' + describe(error));
      return;
    }
    Promise.resolve(reading).then(
      (buffer) => {
        const body = method === 'HEAD' ? new Uint8Array(0) : new Uint8Array(buffer);
        __serve_respond(id, response.status, response.statusText || '', headersText, body);
      },
      (error) => {
        __serve_fail(id, 'reading the response body failed: ' + describe(error));
      },
    );
  }

  function serve(target) {
    let fn = target;
    if (target !== null && typeof target === 'object') {
      fn = target.fetch;
    }
    if (typeof fn !== 'function') {
      throw new TypeError('Eta.serve needs a function or an object with a fetch method');
    }
    handler = fn;
    return { fetch: fn };
  }

  globalThis.Eta = { serve };
  globalThis.__serve_module = { serve };
})();
