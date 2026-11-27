(function () {
  // xmas:realm, a source string evaluated as a module. The compiling is native
  // (src/modules/realm.rs); what is here is the shape a caller touches.
  //
  // Why a dynamic import rather than a host call: the engine evaluates a module itself, and that
  // evaluation waits for the module's own top-level await. A host function is synchronous, so it
  // could only ever answer "started". The promise an import answers with settles when the module
  // has finished, which is what an evaluation means.
  //
  // The specifier is passed on exactly as __realm_load named it. The loader checks its own table
  // before it tries the disk, so the name lands on the source that was just compiled; giving it a
  // './' prefix or escaping it would stop it matching the key.

  function asModule(name) {
    return import(name);
  }

  // Plain fields, not #private: the same shape the other preludes keep, and it survives being
  // read by a tool that parses this file as a class body.
  const NAME = new WeakMap();
  const KILLED = new WeakMap();

  class Realm {
    constructor(options) {
      NAME.set(this, (options && options.name) || 'realm');
      KILLED.set(this, false);
    }

    get name() {
      return NAME.get(this);
    }

    get killed() {
      return KILLED.get(this);
    }

    /**
     * `source` as the module `name`, linked and evaluated.
     *
     * Resolves once the module has finished, top-level await included, with its default export --
     * undefined for a module that has none, as eta's own realm answers.
     */
    evaluate(source, options) {
      if (KILLED.get(this)) {
        throw new Error('realm "' + NAME.get(this) + '" was killed');
      }
      if (typeof source !== 'string') {
        throw new TypeError('evaluate: source must be a string');
      }
      const name = (options && options.identifier) || 'esm.js';
      const where = __realm_load(name, source);
      return asModule(where).then(function (namespace) {
        return {
          value: Object.hasOwn(namespace, 'default') ? namespace.default : undefined,
          identifier: where,
        };
      });
    }

    /** Abandons the realm: later calls throw. Nothing already running is interrupted. */
    kill() {
      KILLED.set(this, true);
    }
  }

  function createRealm(options) {
    return new Realm(options);
  }

  globalThis.__realm_module = { Realm, createRealm, default: createRealm };
})();
