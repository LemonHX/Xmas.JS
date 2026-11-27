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
  const ID = new WeakMap();

  // A run that was waiting on a killed realm has a promise nobody will settle: the engine threw
  // out of the module's evaluation, so no import answer is coming. Keyed by realm id, so killing
  // one realm leaves the evaluations of every other realm alone.
  const WAITING = new Map();

  class Realm {
    constructor(options) {
      NAME.set(this, (options && options.name) || 'realm');
      ID.set(this, __realm_add());
    }

    get name() {
      return NAME.get(this);
    }

    get killed() {
      return !__realm_alive(ID.get(this));
    }

    /**
     * `source` as the module `name`, linked and evaluated.
     *
     * Resolves once the module has finished, top-level await included, with its default export --
     * undefined for a module that has none, as eta's own realm answers.
     */
    evaluate(source, options) {
      const id = ID.get(this);
      if (!__realm_alive(id)) {
        throw new Error('realm "' + NAME.get(this) + '" was killed');
      }
      if (typeof source !== 'string') {
        throw new TypeError('evaluate: source must be a string');
      }
      const name = (options && options.identifier) || 'esm.js';
      const where = __realm_load(name, source);

      // From here until the evaluation settles the engine is running this realm's code, and the
      // interrupt answers by looking at that. It has to be said before the import, because the
      // import is where the loop runs.
      const entry = { reject: null, id };
      const waiting = new Promise(function (_, reject) {
        entry.reject = reject;
      });
      let pending = WAITING.get(id);
      if (!pending) WAITING.set(id, (pending = new Set()));
      pending.add(entry);

      const done = function () {
        pending.delete(entry);
        if (pending.size === 0) WAITING.delete(id);
        __realm_end();
      };

      __realm_begin(id);
      const settled = asModule(where).then(function (namespace) {
        done();
        return {
          value: Object.hasOwn(namespace, 'default') ? namespace.default : undefined,
          identifier: where,
        };
      }, function (error) {
        done();
        throw error;
      });
      // Whichever comes first: the module finishing, or the realm being killed under it.
      return Promise.race([settled, waiting]);
    }

    /**
     * Makes `exports` the module `specifier`: an import of that name inside this realm sees each
     * value under its own name, and `default` as the default export.
     *
     * The table belongs to the driver, so the name belongs to every evaluation in it. One driver is
     * one agent, and a name two agents both want ("eta") is two drivers rather than one name with a
     * suffix.
     *
     * An import in this same turn already sees it. An import of a name provided before, which the
     * engine holds a namespace for, sees this one too: the definition that replaces it is applied
     * before the next job runs.
     */
    provide(specifier, exports) {
      const id = ID.get(this);
      if (!__realm_alive(id)) {
        throw new Error('realm "' + NAME.get(this) + '" was killed');
      }
      if (typeof specifier !== 'string' || specifier === '') {
        throw new TypeError('provide: a module needs a name');
      }
      if (exports === null || typeof exports !== 'object') {
        throw new TypeError('provide: exports must be an object');
      }
      __realm_provide(specifier, exports);
    }

    /**
     * Stops the realm: code running now is interrupted on its next statement, later calls throw,
     * and an evaluation waiting on it is rejected rather than left hanging.
     */
    kill() {
      const id = ID.get(this);
      __realm_kill(id);
      const reason = new Error('realm "' + NAME.get(this) + '" was killed');
      const pending = WAITING.get(id);
      if (pending) {
        WAITING.delete(id);
        for (const entry of pending) entry.reject(reason);
      }
    }
  }

  function createRealm(options) {
    return new Realm(options);
  }

  /**
   * Rejects every evaluation still waiting, saying why. Called by the driver when an interrupt
   * stopped a run: the module's evaluation was thrown out of, so no import answer is coming and
   * whoever awaited it would wait forever without this.
   */
  function abandon(reason) {
    let first = null;
    for (const [id, pending] of WAITING) {
      WAITING.delete(id);
      for (const entry of pending) {
        entry.reject(reason instanceof Error ? reason : new Error(String(reason)));
        if (!first) first = entry;
      }
    }
    __realm_end();
    return first !== null;
  }

  globalThis.__realm_module = { abandon: abandon, createRealm: createRealm, Realm: Realm, default: createRealm };
})();
