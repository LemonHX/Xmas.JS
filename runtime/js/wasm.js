'use strict';

// An IIFE: this file is evaluated as a script, so its top-level bindings would
// land on the global object and collide with the next module's.
(function () {
  // WebAssembly, the standard JS API. Every Module, Instance, Memory, Table and Global is a host
  // object holding the Rust side of it; this file owns the prototypes, the argument shapes and the
  // three error classes. Compiling, instantiating and calling is __wasm_*.

  const HANDLE = Symbol('wasm.handle');

  // The three error classes. The engine knows only the built-in error kinds, so these are plain
  // subclasses of Error here and the Rust side constructs them by walking the WebAssembly object.
  class CompileError extends Error {
    constructor(message) {
      super(message === undefined ? '' : String(message));
    }
  }
  Object.defineProperty(CompileError, 'name', { value: 'CompileError' });
  Object.defineProperty(CompileError.prototype, 'name', {
    value: 'CompileError',
    writable: true,
    configurable: true,
  });

  class LinkError extends Error {
    constructor(message) {
      super(message === undefined ? '' : String(message));
    }
  }
  Object.defineProperty(LinkError, 'name', { value: 'LinkError' });
  Object.defineProperty(LinkError.prototype, 'name', {
    value: 'LinkError',
    writable: true,
    configurable: true,
  });

  class RuntimeError extends Error {
    constructor(message) {
      super(message === undefined ? '' : String(message));
    }
  }
  Object.defineProperty(RuntimeError, 'name', { value: 'RuntimeError' });
  Object.defineProperty(RuntimeError.prototype, 'name', {
    value: 'RuntimeError',
    writable: true,
    configurable: true,
  });

  // The bytes of a BufferSource as a Uint8Array over them. The Rust side copies them out of the
  // view, not out of the engine's buffer: a shared or detached buffer has no bytes to hand over.
  function bytesOf(source) {
    if (source instanceof ArrayBuffer) {
      return new Uint8Array(source);
    }
    if (ArrayBuffer.isView(source)) {
      return new Uint8Array(source.buffer, source.byteOffset, source.byteLength);
    }
    if (source instanceof Module) {
      throw new TypeError('a WebAssembly.Module was given where bytes were expected');
    }
    throw new TypeError('expected a BufferSource of WebAssembly bytes');
  }

  class Module {
    constructor(bytes) {
      if (!new.target) {
        throw new TypeError("Class constructor Module cannot be invoked without 'new'");
      }
      const handle = __wasm_compile(bytesOf(bytes));
      Object.defineProperty(this, HANDLE, { value: handle });
    }

    static customSections() {
      // WNJR's parser does not keep a section's raw bytes, so there is nothing honest to return.
      return [];
    }

    static exports(moduleObject) {
      if (!(moduleObject instanceof Module)) {
        throw new TypeError('expected a WebAssembly.Module');
      }
      return [];
    }

    static imports(moduleObject) {
      if (!(moduleObject instanceof Module)) {
        throw new TypeError('expected a WebAssembly.Module');
      }
      return [];
    }
  }

  Object.defineProperty(Module.prototype, Symbol.toStringTag, {
    value: 'WebAssembly.Module',
    configurable: true,
  });

  // One JS function per exported function, closing over the instance and the export name. The
  // spec has no callable object of its own for these, so a closure is the honest shape.
  function exportedFunction(instance, name) {
    const fn = function (...args) {
      return __wasm_call(instance[HANDLE], name, args);
    };
    Object.defineProperty(fn, 'name', { value: name });
    return fn;
  }

  // The exports of an instance, as the object the spec says instance.exports is. An export whose
  // kind has no wrapper yet is left out rather than faked.
  function exportsOf(instance) {
    const exports = {};
    for (const [name, kind] of __wasm_exports(instance[HANDLE])) {
      let value;
      switch (kind) {
        case 'function':
          value = exportedFunction(instance, name);
          break;
        case 'memory':
          value = memoryOf(instance, name);
          break;
        case 'table':
          value = tableOf(instance, name);
          break;
        case 'global':
          value = globalOf(instance, name);
          break;
        default:
          continue;
      }
      if (value === undefined) {
        continue;
      }
      Object.defineProperty(exports, name, {
        value,
        writable: true,
        enumerable: true,
        configurable: true,
      });
    }
    return exports;
  }

  const PENDING = Symbol('wasm.exportsPending');

  class Instance {
    constructor(module, importObject) {
      if (!new.target) {
        throw new TypeError("Class constructor Instance cannot be invoked without 'new'");
      }
      if (!(module instanceof Module)) {
        throw new TypeError('expected a WebAssembly.Module');
      }
      const handle = __wasm_instantiate(
        module[HANDLE],
        importObject === undefined ? {} : importObject,
      );
      Object.defineProperty(this, HANDLE, { value: handle });
      Object.defineProperty(this, PENDING, { value: true, writable: true });
    }
  }

  Object.defineProperty(Instance.prototype, 'exports', {
    get() {
      if (this[PENDING]) {
        // Built on the first read: building it asks the engine for the export list.
        const exports = exportsOf(this);
        Object.defineProperty(this, 'exports', {
          value: exports,
          writable: false,
          enumerable: true,
          configurable: true,
        });
        this[PENDING] = false;
      }
      return this.exports;
    },
    configurable: true,
  });

  // Placeholders until Memory, Table and Global land: an export of those kinds is left out of
  // exports rather than given a wrapper that lies about what it does.
  function memoryOf() {
    return undefined;
  }
  function tableOf() {
    return undefined;
  }
  function globalOf() {
    return undefined;
  }

  Object.defineProperty(Instance.prototype, Symbol.toStringTag, {
    value: 'WebAssembly.Instance',
    configurable: true,
  });

  // The spec has both a constructor and this: instantiate() is the promise-shaped one, and it also
  // takes bytes, which it compiles first. It resolves with { module, instance } either way.
  function instantiate(source, importObject) {
    let module;
    try {
      module = source instanceof Module ? source : new Module(source);
    } catch (error) {
      return Promise.reject(error);
    }
    try {
      return Promise.resolve({ module, instance: new Instance(module, importObject) });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  function validate(bytes) {
    return __wasm_validate(bytesOf(bytes));
  }

  function compile(bytes) {
    // The spec resolves with the Module, and rejects with a CompileError, which __wasm_compile
    // throws.
    try {
      return Promise.resolve(new Module(bytes));
    } catch (error) {
      return Promise.reject(error);
    }
  }

  function compileStreaming(source) {
    return Promise.resolve(source).then((response) => {
      if (typeof Response === 'function' && response instanceof Response) {
        const type = response.headers.get('content-type');
        if (type !== 'application/wasm') {
          throw new TypeError(
            "compileStreaming: expected a Content-Type of 'application/wasm', got " +
              (type === null ? 'none' : JSON.stringify(type)),
          );
        }
        return response.arrayBuffer();
      }
      return source;
    }).then((bytes) => new Module(bytes));
  }

  const WebAssembly = {
    CompileError,
    Instance,
    LinkError,
    Module,
    RuntimeError,
    compile,
    compileStreaming,
    instantiate,
    validate,
  };

  Object.defineProperty(WebAssembly, Symbol.toStringTag, {
    value: 'WebAssembly',
    configurable: true,
  });

  globalThis.WebAssembly = WebAssembly;
  globalThis.__wasm = { WebAssembly, default: WebAssembly, bytesOf, Module };

})();
