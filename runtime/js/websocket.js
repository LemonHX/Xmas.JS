// websocket: the browser-shaped WebSocket a handler gets from Xmas.upgradeWebSocket.
//
// The frames, the masking and the control frames are Rust (host/websocket.rs). This file is the
// object a handler sees: readyState, onopen, onmessage, onclose, send and close, and one read loop
// that asks for the next message as soon as the last one is handled.

(function () {
  'use strict';

  const CONNECTING = 0;
  const OPEN = 1;
  const CLOSING = 2;
  const CLOSED = 3;

  // The opcode byte the runtime puts in front of every message it hands over.
  const OP_OPEN = 0;
  const OP_TEXT = 1;
  const OP_BINARY = 2;
  const OP_CLOSE = 8;

  const describe = (error) => {
    if (error === null || error === undefined) return String(error);
    if (typeof error.message === 'string' && error.message !== '') return String(error.message);
    return String(error);
  };

  class WebSocket {
    constructor(id) {
      this._id = id;
      this._listeners = { open: [], message: [], close: [], error: [] };
      this._asked = null;

      this.readyState = CONNECTING;
      this.onopen = null;
      this.onmessage = null;
      this.onerror = null;
      this.onclose = null;
      this.url = '';
      this.protocol = '';
      this.extensions = '';
      // deno hands bytes over as an ArrayBuffer by default and this is what that flag is called.
      this.binaryType = 'arraybuffer';

      this._pump();
    }

    addEventListener(type, listener) {
      const list = this._listeners[type];
      if (list !== undefined && list.indexOf(listener) < 0) list.push(listener);
    }

    removeEventListener(type, listener) {
      const list = this._listeners[type];
      if (list === undefined) return;
      const at = list.indexOf(listener);
      if (at >= 0) list.splice(at, 1);
    }

    send(data) {
      if (this.readyState !== OPEN) {
        throw new Error('the socket is not open');
      }
      if (typeof data === 'string') {
        __ws_send(this._id, data, false);
        return;
      }
      __ws_send(this._id, data, true);
    }

    close(code, reason) {
      if (this.readyState >= CLOSING) return;
      const value = code === undefined ? 1000 : Number(code);
      this._asked = { code: value, reason: reason === undefined ? '' : String(reason) };
      this.readyState = CLOSING;
      __ws_close(this._id, value, this._asked.reason);
    }

    _pump() {
      __ws_read(this._id).then(
        (result) => {
          if (result === 'closed') {
            this._finish(1006, '', false);
            return;
          }
          const message = new Uint8Array(result);
          const opcode = message[0];
          const payload = message.subarray(1);
          if (opcode === OP_OPEN) {
            this.readyState = OPEN;
            this._emit('open', { type: 'open' });
            this._pump();
            return;
          }
          if (opcode === OP_TEXT) {
            this._emit('message', { type: 'message', data: __bytes_to_text(payload, 'utf8') });
            this._pump();
            return;
          }
          if (opcode === OP_BINARY) {
            // A copy, because the message is a window on a buffer the runtime will reuse.
            this._emit('message', { type: 'message', data: payload.slice().buffer });
            this._pump();
            return;
          }
          if (opcode === OP_CLOSE) {
            const code = payload.length >= 2 ? (payload[0] << 8) | payload[1] : 1005;
            this._finish(code, '', true);
            return;
          }
          this._pump();
        },
        (error) => {
          this._failed(describe(error));
        },
      );
    }

    _emit(type, event) {
      const handler = this['on' + type];
      if (typeof handler === 'function') {
        try {
          handler.call(this, event);
        } catch (error) {
          console.error('a websocket ' + type + ' handler threw: ' + describe(error));
        }
      }
      const list = this._listeners[type];
      for (let i = 0; i < list.length; i += 1) {
        try {
          list[i].call(this, event);
        } catch (error) {
          console.error('a websocket ' + type + ' listener threw: ' + describe(error));
        }
      }
    }

    _finish(code, reason, clean) {
      if (this.readyState === CLOSED) return;
      const asked = this._asked;
      this.readyState = CLOSED;
      __ws_forget(this._id);
      this._emit('close', {
        type: 'close',
        code: asked === null ? code : asked.code,
        reason: asked === null ? reason : asked.reason,
        wasClean: asked === null ? clean : true,
      });
    }

    _failed(reason) {
      if (this.readyState === CLOSED) return;
      this._emit('error', { type: 'error', message: reason });
      this._finish(1006, reason, false);
    }
  }

  function upgradeWebSocket(request) {
    if (request === null || typeof request !== 'object' || !request.headers) {
      throw new TypeError('upgradeWebSocket needs a Request');
    }
    const upgrade = request.headers.get('upgrade');
    if (upgrade === null || upgrade.toLowerCase() !== 'websocket') {
      throw new TypeError('this request is not a websocket upgrade (upgrade: ' + upgrade + ')');
    }
    const key = request.headers.get('sec-websocket-key');
    if (key === null) {
      throw new TypeError('this request is not a websocket upgrade (no sec-websocket-key)');
    }

    const created = __ws_new(key);
    const cut = created.indexOf('\u0000');
    const socket = new WebSocket(Number(created.slice(0, cut)));
    socket.url = request.url;
    socket.protocol = request.headers.get('sec-websocket-protocol') || '';

    const response = new Response(null, {
      status: 101,
      headers: {
        upgrade: 'websocket',
        connection: 'Upgrade',
        'sec-websocket-accept': created.slice(cut + 1),
      },
    });
    // Not a header and not a body: the server looks for this to decide the connection stops being
    // HTTP after this answer goes out.
    response._upgrade = socket;

    return { socket: socket, response: response };
  }

  globalThis.WebSocket = WebSocket;
  globalThis.__ws_module = { WebSocket, upgradeWebSocket };
  const host = globalThis.Xmas === undefined || globalThis.Xmas === null ? (globalThis.Xmas = {}) : globalThis.Xmas;
  host.upgradeWebSocket = upgradeWebSocket;
})();
