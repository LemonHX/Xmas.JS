//! xmas:sqlite: a synchronous SQLite interface on FrankenSQLite.
#![cfg(feature = "sqlite")]

use xmas_runtime::testkit::{Harness, text_of};

fn run(h: &mut Harness, body: &str) -> String {
    // Modules are evaluated as scripts here, so the module comes in through a global.
    let source = format!(
        "(function(){{ const {{ Database }} = globalThis.__sqlite_module; {body} }})()"
    );
    match h.eval(&source) {
        Ok(value) => text_of(&value).unwrap_or_else(|| format!("{value:?}")),
        Err(error) => panic!("{body} failed: {error}"),
    }
}

#[test]
fn rows_come_back_as_objects_and_arrays() {
    let mut h = Harness::with_modules();
    let out = run(
        &mut h,
        "const db = new Database(':memory:');
         db.exec('CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, score REAL, data BLOB)');
         db.run('INSERT INTO t (name, score, data) VALUES (?, ?, ?)', 'ada', 1.5, new Uint8Array([1,2,3]));
         db.query('INSERT INTO t (name) VALUES ($n)').run({ $n: 'bola' });
         const all = db.query('SELECT * FROM t ORDER BY id').all();
         const one = db.query('SELECT name FROM t WHERE id = ?').get(2);
         const vals = db.query('SELECT id, name FROM t ORDER BY id').values();
         const none = db.query('SELECT * FROM t WHERE id = 99').get();
         return JSON.stringify({ all, one, vals, none });",
    );
    assert_eq!(
        out,
        r#"{"all":[{"id":1,"name":"ada","score":1.5,"data":{"0":1,"1":2,"2":3}},{"id":2,"name":"bola","score":null,"data":null}],"one":{"name":"bola"},"vals":[[1,"ada"],[2,"bola"]],"none":null}"#
    );
}

#[test]
fn run_reports_changes_and_the_last_row_id() {
    let mut h = Harness::with_modules();
    let out = run(
        &mut h,
        "const db = new Database();
         db.exec('CREATE TABLE t (id INTEGER PRIMARY KEY, v INT)');
         const a = db.run('INSERT INTO t (v) VALUES (1), (2)');
         const b = db.run('UPDATE t SET v = 9');
         return JSON.stringify([a, b]);",
    );
    assert_eq!(
        out,
        r#"[{"lastInsertRowid":2,"changes":2},{"lastInsertRowid":2,"changes":2}]"#
    );
}

#[test]
fn preparing_runs_nothing() {
    let mut h = Harness::with_modules();
    let out = run(
        &mut h,
        "const db = new Database();
         db.exec('CREATE TABLE t (v INT)');
         db.prepare('INSERT INTO t VALUES (1)');
         return String(db.query('SELECT count(*) AS n FROM t').get().n);",
    );
    assert_eq!(out, "0");
}

#[test]
fn a_transaction_commits_and_a_throw_rolls_back() {
    let mut h = Harness::with_modules();
    let out = run(
        &mut h,
        "const db = new Database();
         db.exec('CREATE TABLE t (v INT)');
         const add = db.transaction((n) => { for (let i = 0; i < n; i++) db.run('INSERT INTO t VALUES (?)', i); });
         add(3);
         const bad = db.transaction(() => { db.run('INSERT INTO t VALUES (100)'); throw new Error('no'); });
         let thrown = '';
         try { bad(); } catch (e) { thrown = e.message; }
         const n = db.query('SELECT count(*) AS n FROM t').get().n;
         return JSON.stringify({ n, thrown, open: db.inTransaction });",
    );
    assert_eq!(out, r#"{"n":3,"thrown":"no","open":false}"#);
}

#[test]
fn large_integers_are_bigints_with_safe_integers() {
    let mut h = Harness::with_modules();
    let out = run(
        &mut h,
        "const db = new Database(':memory:', { safeIntegers: true });
         db.exec('CREATE TABLE t (v INTEGER)');
         db.run('INSERT INTO t VALUES (?)', 9007199254740993n);
         return String(db.query('SELECT v FROM t').get().v) + typeof db.query('SELECT v FROM t').get().v;",
    );
    assert_eq!(out, "9007199254740993bigint");
}

#[test]
fn errors_are_sqlite_errors_and_a_closed_database_refuses() {
    let mut h = Harness::with_modules();
    let out = run(
        &mut h,
        "const db = new Database();
         let bad = '';
         try { db.query('SELEKT 1'); } catch (e) { bad = e.name; }
         db.close();
         let closed = '';
         try { db.query('SELECT 1').all(); } catch (e) { closed = e.message; }
         return JSON.stringify({ bad, closed });",
    );
    assert_eq!(
        out,
        r#"{"bad":"SQLiteError","closed":"Cannot use a closed database"}"#
    );
}

#[test]
fn a_file_database_keeps_its_rows() {
    let dir = std::env::temp_dir().join(format!("xmas-sqlite-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.db");
    let _ = std::fs::remove_file(&path);
    let file = path.to_string_lossy().replace('\\', "/");
    let mut h = Harness::with_modules();
    run(
        &mut h,
        &format!(
            "const db = new Database('{file}');
             db.exec('CREATE TABLE t (v TEXT)'); db.run('INSERT INTO t VALUES (?)', 'kept'); db.close(); return '';"
        ),
    );
    let out = run(
        &mut h,
        &format!("const db = new Database('{file}'); return db.query('SELECT v FROM t').get().v;"),
    );
    assert_eq!(out, "kept");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_module_is_importable_by_name() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "app",
            "import D, { Database } from 'xmas:sqlite';
             const db = new Database(':memory:');
             db.exec('CREATE TABLE t (v TEXT)');
             db.run('INSERT INTO t VALUES (?)', 'hi');
             export const out = JSON.stringify(db.query('SELECT v FROM t').all()) + (D === Database);",
        )
        .unwrap();
    let value = h.property(&ns, "out").unwrap();
    assert_eq!(text_of(&value).unwrap(), r#"[{"v":"hi"}]true"#);
}
