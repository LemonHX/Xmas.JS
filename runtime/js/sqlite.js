(function () {
  // xmas:sqlite, a synchronous SQLite interface. The engine is native (src/modules/sqlite.rs);
  // what is here is the shape scripts touch: Database, Statement, transactions, parameter binding.

  class SQLiteError extends Error {
    constructor(message) {
      super(message);
      this.name = 'SQLiteError';
    }
  }

  const call = (fn, ...args) => {
    try {
      return fn(...args);
    } catch (error) {
      if (error instanceof SQLiteError) throw error;
      throw new SQLiteError(String(error && error.message !== undefined ? error.message : error));
    }
  };

  const isBytes = (v) => v instanceof ArrayBuffer || ArrayBuffer.isView(v);

  function encode(value) {
    if (value === undefined || value === null) return null;
    switch (typeof value) {
      case 'boolean':
        return value ? 1 : 0;
      case 'bigint':
        return { $i: value.toString() };
      case 'number':
        return Number.isFinite(value) ? value : null;
      case 'string':
        return value;
    }
    if (isBytes(value)) {
      const view = value instanceof ArrayBuffer ? new Uint8Array(value) : new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
      return { $b: Buffer.from(view).toString('base64') };
    }
    throw new SQLiteError('Binding expected string, TypedArray, boolean, number, bigint or null');
  }

  function params(args) {
    if (args.length === 1 && args[0] !== null && typeof args[0] === 'object' && !isBytes(args[0])) {
      const source = args[0];
      if (Array.isArray(source)) return JSON.stringify(source.map(encode));
      const named = {};
      for (const key of Object.keys(source)) named[key] = encode(source[key]);
      return JSON.stringify(named);
    }
    return JSON.stringify(args.map(encode));
  }

  const constants = {
    SQLITE_OPEN_READONLY: 0x1,
    SQLITE_OPEN_READWRITE: 0x2,
    SQLITE_OPEN_CREATE: 0x4,
  };

  class Statement {
    #db;
    #sql;
    #class = null;
    #columns = [];
    #done = false;
    constructor(db, sql) {
      this.#db = db;
      this.#sql = sql;
      // Prepare once now, so a syntax error is thrown where the statement is made.
      this.#columns = db._run(sql, '[]', true).columns;
    }
    get native() {
      return this;
    }
    get columnNames() {
      return this.#columns.slice();
    }
    get paramsCount() {
      return (this.#sql.match(/\?|[:@$][A-Za-z_]\w*/g) || []).length;
    }
    toString() {
      return this.#sql;
    }
    as(Class) {
      this.#class = Class;
      return this;
    }
    #answer(args) {
      if (this.#done) throw new SQLiteError('Statement has finalized');
      return this.#db._run(this.#sql, params(args), false);
    }
    #shape(columns, row) {
      const out = {};
      columns.forEach((name, i) => {
        out[name] = row[i];
      });
      if (this.#class) Object.setPrototypeOf(out, this.#class.prototype);
      return out;
    }
    #cells(row) {
      return row.map((cell) => this.#db._cell(cell));
    }
    all(...args) {
      const answer = this.#answer(args);
      return answer.rows.map((row) => this.#shape(answer.columns, this.#cells(row)));
    }
    get(...args) {
      const answer = this.#answer(args);
      return answer.rows.length === 0 ? null : this.#shape(answer.columns, this.#cells(answer.rows[0]));
    }
    values(...args) {
      return this.#answer(args).rows.map((row) => this.#cells(row));
    }
    run(...args) {
      const answer = this.#answer(args);
      return { lastInsertRowid: this.#db._cell(answer.lastInsertRowid), changes: answer.changes };
    }
    *iterate(...args) {
      for (const row of this.all(...args)) yield row;
    }
    [Symbol.iterator]() {
      return this.iterate();
    }
    finalize() {
      this.#done = true;
    }
  }

  class Database {
    #id = 0;
    #safe;
    #cache = new Map();
    #depth = 0;
    constructor(filename = ':memory:', options = {}) {
      if (typeof options === 'number') {
        options = {
          readonly: (options & constants.SQLITE_OPEN_READONLY) !== 0,
          create: (options & constants.SQLITE_OPEN_CREATE) !== 0,
          readwrite: (options & constants.SQLITE_OPEN_READWRITE) !== 0,
        };
      }
      this.#safe = !!options.safeIntegers;
      const mode = options.readonly ? 'readonly' : options.create === false ? 'readwrite' : 'create';
      this.filename = String(filename);
      this.#id = call(__sqlite_open, this.filename, mode);
    }
    get inTransaction() {
      return this.#depth > 0;
    }
    get handle() {
      return this.#id;
    }
    _cell(cell) {
      if (cell !== null && typeof cell === 'object') {
        if ('$i' in cell) return this.#safe ? BigInt(cell.$i) : Number(cell.$i);
        if ('$b' in cell) return new Uint8Array(Buffer.from(cell.$b, 'base64'));
      }
      return cell;
    }
    _run(sql, args, probe) {
      if (!this.#id) throw new SQLiteError('Cannot use a closed database');
      // A probe only prepares: it reads the column names and runs nothing.
      if (probe) return { columns: JSON.parse(call(__sqlite_columns, this.#id, sql)) };
      return JSON.parse(call(__sqlite_run, this.#id, sql, args));
    }
    query(sql) {
      let statement = this.#cache.get(sql);
      if (!statement) {
        statement = new Statement(this, sql);
        this.#cache.set(sql, statement);
      }
      return statement;
    }
    prepare(sql, ...args) {
      const statement = new Statement(this, sql);
      return args.length ? Object.assign(statement, {}) : statement;
    }
    run(sql, ...args) {
      const answer = this._run(sql, params(args), false);
      return { lastInsertRowid: this._cell(answer.lastInsertRowid), changes: answer.changes };
    }
    exec(sql, ...args) {
      if (args.length === 0) {
        if (!this.#id) throw new SQLiteError('Cannot use a closed database');
        call(__sqlite_exec, this.#id, sql);
        return;
      }
      return this.run(sql, ...args);
    }
    transaction(fn) {
      const make = (begin) => (...args) => {
        const nested = this.#depth > 0;
        const name = 'xmas_sp_' + this.#depth;
        this.exec(nested ? 'SAVEPOINT ' + name : begin);
        this.#depth += 1;
        try {
          const result = fn.apply(this, args);
          this.#depth -= 1;
          this.exec(nested ? 'RELEASE ' + name : 'COMMIT');
          return result;
        } catch (error) {
          this.#depth -= 1;
          this.exec(nested ? 'ROLLBACK TO ' + name + '; RELEASE ' + name : 'ROLLBACK');
          throw error;
        }
      };
      const wrapped = make('BEGIN');
      wrapped.deferred = make('BEGIN DEFERRED');
      wrapped.immediate = make('BEGIN IMMEDIATE');
      wrapped.exclusive = make('BEGIN EXCLUSIVE');
      return wrapped;
    }
    close() {
      if (!this.#id) return;
      __sqlite_close(this.#id);
      this.#id = 0;
      this.#cache.clear();
    }
    [Symbol.dispose]() {
      this.close();
    }
  }

  globalThis.__sqlite_module = { Database, Statement, SQLiteError, constants, default: Database };
})();
