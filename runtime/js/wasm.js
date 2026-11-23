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

  class Memory {
    constructor(descriptor) {
      if (!new.target) {
        throw new TypeError("Class constructor Memory cannot be invoked without 'new'");
      }
      if (descriptor === null || typeof descriptor !== 'object') {
        throw new TypeError('expected a memory descriptor');
      }
      const initial = descriptor.initial;
      if (typeof initial !== 'number') {
        throw new TypeError('the memory descriptor needs an initial page count');
      }
      const handle = __wasm_new_memory(
        initial,
        descriptor.maximum === undefined ? -1 : descriptor.maximum,
        descriptor.shared === true,
      );
      installMemory(this, handle);
    }
  }

  // The one place a Memory object is made from a Rust slot: both the constructor and an
  // instance's exports go through it.
  function installMemory(target, handle) {
    Object.defineProperty(target, HANDLE, { value: handle, configurable: true });
  }

  function memoryFromSlot(handle) {
    const memory = Object.create(Memory.prototype);
    installMemory(memory, handle);
    return memory;
  }

  Object.defineProperty(Memory.prototype, 'buffer', {
    get() {
      return __wasm_memory_buffer(this[HANDLE]);
    },
    configurable: true,
  });

  Object.defineProperty(Memory.prototype, 'grow', {
    value: function grow(delta) {
      if (typeof delta !== 'number') {
        throw new TypeError('grow takes a number of pages');
      }
      return __wasm_memory_grow(this[HANDLE], delta);
    },
    writable: true,
    configurable: true,
  });

  // The current length in pages, which a grow changes.
  Object.defineProperty(Memory.prototype, 'length', {
    get() {
      return __wasm_memory_length(this[HANDLE]);
    },
    configurable: true,
  });

  Object.defineProperty(Memory.prototype, Symbol.toStringTag, {
    value: 'WebAssembly.Memory',
    configurable: true,
  });

  function memoryOf(instance, name) {
    return memoryFromSlot(__wasm_memory(instance[HANDLE], name));
  }

  // A funcref read out of a table or a global comes back as a Rust slot; the callable the
  // spec promises is a JS function over it, since a reference is not a JS value.
  function callableFromSlot(slot) {
    const fn = function (...args) {
      return __wasm_ref_call(slot, args);
    };
    Object.defineProperty(fn, 'name', { value: '' });
    return fn;
  }

  function fromReference(value) {
    if (value === null || value === undefined) {
      return null;
    }
    if (typeof value === 'object' && value !== null) {
      return callableFromSlot(value);
    }
    return value;
  }

  function tableFromSlot(handle) {
    const table = Object.create(Table.prototype);
    Object.defineProperty(table, HANDLE, { value: handle, configurable: true });
    return table;
  }

  function globalFromSlot(handle) {
    const global = Object.create(Global.prototype);
    Object.defineProperty(global, HANDLE, { value: handle, configurable: true });
    return global;
  }

  class Table {
    constructor(descriptor) {
      if (!new.target) {
        throw new TypeError("Class constructor Table cannot be invoked without 'new'");
      }
      throw new TypeError('constructing a WebAssembly.Table is not supported yet');
    }
  }

  Object.defineProperty(Table.prototype, 'length', {
    get() {
      return __wasm_table_length(this[HANDLE]);
    },
    configurable: true,
  });

  Object.defineProperty(Table.prototype, 'get', {
    value: function get(index) {
      if (typeof index !== 'number') {
        throw new TypeError('get takes an element index');
      }
      return fromReference(__wasm_table_get(this[HANDLE], index));
    },
    writable: true,
    configurable: true,
  });

  Object.defineProperty(Table.prototype, 'set', {
    value: function set(index, value) {
      if (typeof index !== 'number') {
        throw new TypeError('set takes an element index');
      }
      __wasm_table_set(this[HANDLE], index, value === undefined ? null : value);
    },
    writable: true,
    configurable: true,
  });

  Object.defineProperty(Table.prototype, Symbol.toStringTag, {
    value: 'WebAssembly.Table',
    configurable: true,
  });

  class Global {
    constructor(descriptor) {
      if (!new.target) {
        throw new TypeError("Class constructor Global cannot be invoked without 'new'");
      }
      throw new TypeError('constructing a WebAssembly.Global is not supported yet');
    }
  }

  Object.defineProperty(Global.prototype, 'value', {
    get() {
      return fromReference(__wasm_global_value(this[HANDLE]));
    },
    set(value) {
      __wasm_global_set(this[HANDLE], value);
    },
    configurable: true,
  });

  Object.defineProperty(Global.prototype, 'valueOf', {
    value: function valueOf() {
      return this.value;
    },
    writable: true,
    configurable: true,
  });

  Object.defineProperty(Global.prototype, Symbol.toStringTag, {
    value: 'WebAssembly.Global',
    configurable: true,
  });

  function tableOf(instance, name) {
    return tableFromSlot(__wasm_table(instance[HANDLE], name));
  }
  function globalOf(instance, name) {
    return globalFromSlot(__wasm_global(instance[HANDLE], name));
  }

  Object.defineProperty(Instance.prototype, Symbol.toStringTag, {
    value: 'WebAssembly.Instance',
    configurable: true,
  });

  // The spec has both a constructor and this: instantiate() is the promise-shaped one, and it also
  // takes bytes, which it compiles first. Handed a Module and nothing else it resolves with the
  // Instance; in every other shape it resolves with { module, instance }.
  function instantiate(source, importObject) {
    const bare = source instanceof Module && arguments.length < 2;
    let module;
    try {
      module = source instanceof Module ? source : new Module(source);
    } catch (error) {
      return Promise.reject(error);
    }
    try {
      const instance = new Instance(module, importObject);
      return Promise.resolve(bare ? instance : { module, instance });
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

  // Not streaming: the body is collected and compiled in one go, which is the same result by a
  // longer road. Reading it chunk by chunk would buy nothing here -- the engine compiles bytes,
  // and a compiled module is what both of these resolve with either way.
  function instantiateStreaming(source, importObject) {
    return compileStreaming(source).then((module) => {
      try {
        const instance = new Instance(module, importObject);
        return { module, instance };
      } catch (error) {
        throw error;
      }
    });
  }

  const WebAssembly = {
    CompileError,
    Global,
    Instance,
    LinkError,
    Memory,
    Module,
    RuntimeError,
    Table,
    compile,
    compileStreaming,
    instantiate,
    instantiateStreaming,
    validate,
  };

  Object.defineProperty(WebAssembly, Symbol.toStringTag, {
    value: 'WebAssembly',
    configurable: true,
  });

  globalThis.WebAssembly = WebAssembly;
  globalThis.__wasm = { WebAssembly, default: WebAssembly, bytesOf, Module };

})();
